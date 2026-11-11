//! A logging reverse proxy in front of the provider. Every external harness reaches the model
//! through it, so turns and tokens are counted the same way for all of them and priced with
//! Presage's own table.
//!
//! A run talks to `http://127.0.0.1:<port>/r/<run>/v1/...`; the proxy forwards to the provider's
//! base URL with the real key and passes the answer through unchanged, streamed or not.

use presage_core::economy::{peak_multiplier, prices_for};
use presage_core::provider::Usage;
use presage_core::util::{now_ms, one_line};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

const MAX_HEAD: usize = 64 * 1024;
const MAX_BODY: usize = 64 * 1024 * 1024;
/// How long the provider may stay silent in the middle of an answer.
const IDLE: Duration = Duration::from_secs(300);
/// Headers that describe one connection, and the harness's placeholder credentials.
const NOT_FORWARDED: &[&str] = &["host", "connection", "keep-alive", "content-length", "transfer-encoding", "accept-encoding", "expect", "te", "upgrade", "proxy-connection", "authorization", "x-api-key", "api-key"];
const NOT_RETURNED: &[&str] = &["connection", "keep-alive", "content-length", "transfer-encoding"];

/// What one run asked of the provider.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Metered {
    /// Model requests the provider answered. Title and summary requests count like any other.
    pub turns: u32,
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    /// Prompt size of the first request that offers tools, else of the first request: the
    /// harness's system prompt, its tool schemas and the task. A title request is not it.
    pub first_prompt_tokens: u64,
    #[serde(skip)]
    pub(super) first_offered_tools: bool,
    /// Prompt size of the latest request.
    pub context_tokens: u64,
    /// USD at Presage's list prices; meaningful only when `priced`.
    pub cost: f64,
    /// The same had the provider served nothing from its cache.
    pub cost_uncached: f64,
    pub priced: bool,
    /// Time spent waiting for the provider.
    pub model_ms: u64,
    /// Answered requests that reported no usage: their tokens are missing from the sums.
    pub unmetered: u32,
    /// Streamed requests that did not ask for usage, so the proxy asked on their behalf.
    pub usage_injected: u32,
    /// Requests the provider failed or rejected.
    pub failed: u32,
    /// Requests refused here because the run had used up its turns.
    pub refused: u32,
    /// Requests that were not model calls, such as a model listing.
    pub other_requests: u32,
    pub models: Vec<String>,
    /// Set while the latest model request is a failed one, with its HTTP status (0: no answer).
    pub last_error: Option<String>,
    pub last_status: u16,
    /// Every model request in the order it ended, the failed ones included.
    pub calls: Vec<Call>,
}

/// One model request as the proxy saw it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Call {
    /// When it was sent, counted from the start of the run, and how long the answer took.
    pub at_ms: u64,
    pub ms: u64,
    pub model: String,
    pub stream: bool,
    /// How many messages and tool schemas it carried.
    pub messages: usize,
    pub tools: usize,
    /// 200 for an answer, else the provider's status, or 0 when there was none.
    pub status: u16,
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub output_tokens: u64,
}

impl Metered {
    fn add(&mut self, model: &str, tools: bool, u: &Usage) {
        if self.turns == 1 || (tools && !self.first_offered_tools) {
            self.first_prompt_tokens = u.prompt;
            self.first_offered_tools = tools;
        }
        self.prompt_tokens += u.prompt;
        self.cached_tokens += u.cached;
        self.output_tokens += u.output;
        self.reasoning_tokens += u.reasoning;
        self.context_tokens = u.prompt;
        if let Some(p) = prices_for(model) {
            let peak = peak_multiplier(model, now_ms() / 1000);
            let cost = p.cost(u);
            self.cost += cost * peak;
            self.cost_uncached += (cost + u.cached as f64 * (p.miss - p.hit) / 1e6) * peak;
            self.priced = true;
        }
    }
}

/// The running count of one run.
pub struct Meter {
    max_turns: u32,
    opened: Instant,
    state: Mutex<(Metered, u32)>,
    /// Notified when a request is refused because the turns are used up.
    pub capped: Notify,
}

struct Inner {
    upstream: String,
    key: String,
    client: reqwest::Client,
    runs: Mutex<HashMap<String, Arc<Meter>>>,
}

pub struct Proxy {
    inner: Arc<Inner>,
    port: u16,
    accept: JoinHandle<()>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

impl Proxy {
    /// Listens on a free local port and forwards to `upstream`, a base URL such as
    /// `https://host/v1`, authenticating with `key`.
    pub async fn start(upstream: &str, key: &str) -> Result<Proxy, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| format!("could not start the metering proxy: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(30)).build().map_err(|e| e.to_string())?;
        let inner = Arc::new(Inner { upstream: upstream.trim_end_matches('/').to_string(), key: key.to_string(), client, runs: Mutex::new(HashMap::new()) });
        let shared = inner.clone();
        let accept = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let inner = shared.clone();
                tokio::spawn(async move {
                    let _ = serve(&inner, stream).await;
                });
            }
        });
        Ok(Proxy { inner, port, accept })
    }

    /// Starts counting for a run and returns the base URL its harness must use.
    pub fn open(&self, run: &str, max_turns: u32) -> (String, Arc<Meter>) {
        let meter = Arc::new(Meter { max_turns, opened: Instant::now(), state: Mutex::new((Metered::default(), 0)), capped: Notify::new() });
        self.inner.runs.lock().unwrap().insert(run.to_string(), meter.clone());
        (format!("http://127.0.0.1:{}/r/{run}/v1", self.port), meter)
    }

    /// Stops serving the run and returns what it used.
    pub fn close(&self, run: &str) -> Metered {
        let meter = self.inner.runs.lock().unwrap().remove(run);
        meter.map(|m| m.state.lock().unwrap().0.clone()).unwrap_or_default()
    }
}

struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    keep_alive: bool,
}

fn bad(what: &str) -> Error {
    Error::new(ErrorKind::InvalidData, what.to_string())
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Reads until `buf` holds at least `n` bytes.
async fn fill(stream: &mut TcpStream, buf: &mut Vec<u8>, n: usize) -> std::io::Result<()> {
    while buf.len() < n {
        if stream.read_buf(buf).await? == 0 {
            return Err(ErrorKind::UnexpectedEof.into());
        }
    }
    Ok(())
}

/// Takes one CRLF-terminated line off the stream.
async fn line(stream: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<String> {
    loop {
        if let Some(end) = find(buf, b"\r\n") {
            let text = String::from_utf8_lossy(&buf[..end]).into_owned();
            buf.drain(..end + 2);
            return Ok(text);
        }
        if buf.len() > MAX_HEAD {
            return Err(bad("line too long"));
        }
        fill(stream, buf, buf.len() + 1).await?;
    }
}

/// The next request on a connection, or `None` when the client has closed it.
async fn read_request(stream: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<Option<Request>> {
    let end = loop {
        if let Some(end) = find(buf, b"\r\n\r\n") {
            break end;
        }
        if buf.len() > MAX_HEAD {
            return Err(bad("request head too large"));
        }
        if stream.read_buf(buf).await? == 0 {
            return if buf.is_empty() { Ok(None) } else { Err(ErrorKind::UnexpectedEof.into()) };
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    buf.drain(..end + 4);
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, target, version) = (first.next().unwrap_or(""), first.next().ok_or_else(|| bad("no request target"))?, first.next().unwrap_or(""));
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string())).collect();
    let header = |name: &str| headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.to_ascii_lowercase());

    if header("expect").is_some_and(|v| v.contains("100-continue")) {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await?;
    }
    let mut body = Vec::new();
    if header("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        loop {
            let size_line = line(stream, buf).await?;
            let size = usize::from_str_radix(size_line.split(';').next().unwrap_or("").trim(), 16).map_err(|_| bad("bad chunk size"))?;
            if size == 0 {
                while !line(stream, buf).await?.is_empty() {}
                break;
            }
            if body.len() + size > MAX_BODY {
                return Err(bad("request body too large"));
            }
            fill(stream, buf, size + 2).await?;
            body.extend(buf.drain(..size));
            buf.drain(..2);
        }
    } else if let Some(length) = header("content-length") {
        let length: usize = length.trim().parse().map_err(|_| bad("bad content-length"))?;
        if length > MAX_BODY {
            return Err(bad("request body too large"));
        }
        fill(stream, buf, length).await?;
        body.extend(buf.drain(..length));
    }
    let keep_alive = version == "HTTP/1.1" && !header("connection").is_some_and(|v| v.contains("close"));
    Ok(Some(Request { method: method.to_string(), target: target.to_string(), headers, body, keep_alive }))
}

fn reason(status: u16) -> &'static str {
    reqwest::StatusCode::from_u16(status).ok().and_then(|s| s.canonical_reason()).unwrap_or("")
}

fn connection(keep_alive: bool) -> &'static str {
    if keep_alive { "keep-alive" } else { "close" }
}

/// Answers with an error of the proxy's own, in the shape of an OpenAI error.
async fn refuse(stream: &mut TcpStream, status: u16, code: &str, message: &str, keep_alive: bool) -> std::io::Result<()> {
    let body = json!({"error": {"message": message, "type": "invalid_request_error", "param": null, "code": code}}).to_string();
    let head = format!("HTTP/1.1 {status} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: {}\r\n\r\n", reason(status), body.len(), connection(keep_alive));
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.flush().await
}

fn u64_at(v: &Value, path: &[&str]) -> Option<u64> {
    path.iter().try_fold(v, |cur, key| cur.get(key))?.as_u64()
}

/// Reads a `usage` object the way Presage's own provider client does.
fn parse_usage(u: &Value) -> Usage {
    Usage {
        prompt: u64_at(u, &["prompt_tokens"]).or_else(|| u64_at(u, &["input_tokens"])).unwrap_or(0),
        cached: u64_at(u, &["prompt_tokens_details", "cached_tokens"])
            .or_else(|| u64_at(u, &["prompt_cache_hit_tokens"]))
            .or_else(|| u64_at(u, &["input_tokens_details", "cached_tokens"]))
            .or_else(|| u64_at(u, &["cache_read_input_tokens"]))
            .unwrap_or(0),
        output: u64_at(u, &["completion_tokens"]).or_else(|| u64_at(u, &["output_tokens"])).unwrap_or(0),
        reasoning: u64_at(u, &["completion_tokens_details", "reasoning_tokens"])
            .or_else(|| u64_at(u, &["output_tokens_details", "reasoning_tokens"]))
            .or_else(|| u64_at(u, &["reasoning_tokens"]))
            .unwrap_or(0),
    }
}

/// The usage a response body or stream event carries: at its top for chat completions, under
/// `response` in the events of the Responses API.
fn usage_in(v: &Value) -> Option<Usage> {
    [v.get("usage"), v.get("response").and_then(|r| r.get("usage"))].into_iter().flatten().find(|u| u.is_object()).map(parse_usage)
}

/// Watches a response go by and keeps the usage it reports. In a stream the last report wins.
struct Scanner {
    sse: bool,
    buf: Vec<u8>,
    usage: Option<Usage>,
}

impl Scanner {
    fn new(sse: bool) -> Self {
        Self { sse, buf: Vec::new(), usage: None }
    }

    fn feed(&mut self, chunk: &[u8]) {
        if self.buf.len() + chunk.len() > MAX_BODY {
            return;
        }
        self.buf.extend_from_slice(chunk);
        if self.sse {
            while let Some(end) = self.buf.iter().position(|b| *b == b'\n') {
                let event: Vec<u8> = self.buf.drain(..=end).collect();
                self.event(&event);
            }
        }
    }

    fn event(&mut self, line: &[u8]) {
        let Some(data) = line.strip_prefix(b"data:") else { return };
        if let Some(usage) = serde_json::from_slice::<Value>(data).ok().as_ref().and_then(usage_in) {
            self.usage = Some(usage);
        }
    }

    fn finish(mut self) -> Option<Usage> {
        let rest = std::mem::take(&mut self.buf);
        if self.sse {
            self.event(&rest);
        } else if let Ok(v) = serde_json::from_slice::<Value>(&rest) {
            self.usage = usage_in(&v);
        } else {
            // A stream sent without the event-stream content type.
            rest.split(|b| *b == b'\n').for_each(|l| self.event(l));
        }
        self.usage
    }
}

/// Makes a streamed chat completion report its usage. True when the body had to be changed.
fn ask_for_usage(body: &mut Value) -> bool {
    let asked = body.get("stream_options").and_then(|o| o.get("include_usage")).and_then(Value::as_bool) == Some(true);
    if body.get("stream").and_then(Value::as_bool) != Some(true) || asked {
        return false;
    }
    if !body.get("stream_options").is_some_and(Value::is_object) {
        body["stream_options"] = json!({});
    }
    body["stream_options"]["include_usage"] = json!(true);
    true
}

/// How a forwarded request ended.
struct Relayed {
    /// The provider's answer in full: `Ok` with the usage it reported, `Err` with what went wrong.
    answer: Result<Option<Usage>, (u16, String)>,
    /// The connection to the harness can take another request.
    reusable: bool,
}

/// Sends the request on and streams the answer back, chunk by chunk.
async fn relay(inner: &Inner, stream: &mut TcpStream, req: &Request, url: &str, body: Vec<u8>) -> Relayed {
    let method = reqwest::Method::from_bytes(req.method.as_bytes()).unwrap_or(reqwest::Method::GET);
    let mut out = inner.client.request(method, url).bearer_auth(&inner.key).body(body);
    for (name, value) in req.headers.iter().filter(|(n, _)| !NOT_FORWARDED.contains(&n.as_str())) {
        out = out.header(name, value);
    }
    let mut resp = match out.send().await {
        Ok(resp) => resp,
        Err(e) => {
            let message = format!("network error: {e}");
            let sent = refuse(stream, 502, "bench_upstream_unreachable", &message, req.keep_alive).await;
            return Relayed { answer: Err((0, message)), reusable: sent.is_ok() };
        }
    };
    let status = resp.status();
    let sse = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|v| v.contains("event-stream"));
    let bodiless = req.method == "HEAD" || matches!(status.as_u16(), 204 | 304);

    let mut head = format!("HTTP/1.1 {} {}\r\n", status.as_u16(), reason(status.as_u16())).into_bytes();
    for (name, value) in resp.headers().iter().filter(|(n, _)| !NOT_RETURNED.contains(&n.as_str())) {
        head.extend_from_slice(name.as_str().as_bytes());
        head.extend_from_slice(b": ");
        head.extend_from_slice(value.as_bytes());
        head.extend_from_slice(b"\r\n");
    }
    head.extend_from_slice(format!("{}connection: {}\r\n\r\n", if bodiless { "" } else { "transfer-encoding: chunked\r\n" }, connection(req.keep_alive)).as_bytes());
    let mut client_there = stream.write_all(&head).await.is_ok();

    let mut scanner = Scanner::new(sse);
    let mut broken = None;
    while client_there {
        let chunk = match tokio::time::timeout(IDLE, resp.chunk()).await {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => break,
            Ok(Err(e)) => {
                broken = Some(format!("network error: {e}"));
                break;
            }
            Err(_) => {
                broken = Some(format!("network error: the provider sent nothing for {}s", IDLE.as_secs()));
                break;
            }
        };
        // An empty chunk would end the chunked body.
        if chunk.is_empty() || bodiless {
            continue;
        }
        scanner.feed(&chunk);
        let framed = async {
            stream.write_all(format!("{:x}\r\n", chunk.len()).as_bytes()).await?;
            stream.write_all(&chunk).await?;
            stream.write_all(b"\r\n").await?;
            stream.flush().await
        };
        client_there = framed.await.is_ok();
    }
    // An answer cut short is left without its end, so that the harness sees it as cut short.
    let complete = client_there && broken.is_none();
    let ended = complete && (bodiless || stream.write_all(b"0\r\n\r\n").await.is_ok()) && stream.flush().await.is_ok();

    let answer = if let Some(message) = broken {
        Err((0, message))
    } else if !status.is_success() {
        let said = String::from_utf8_lossy(&scanner.buf).into_owned();
        Err((status.as_u16(), format!("provider returned HTTP {}: {}", status.as_u16(), one_line(&said, 200))))
    } else {
        Ok(scanner.finish())
    };
    Relayed { answer, reusable: ended && req.keep_alive }
}

/// Handles one request. Returns whether the connection can take another.
async fn handle(inner: &Inner, stream: &mut TcpStream, req: Request) -> std::io::Result<bool> {
    let (path, query) = req.target.split_once('?').map_or((req.target.as_str(), ""), |(p, q)| (p, q));
    let route = path.strip_prefix("/r/").and_then(|rest| rest.split_once('/')).and_then(|(run, rest)| Some((run, rest.strip_prefix("v1")?)));
    let Some((run, rest)) = route.filter(|(_, rest)| rest.is_empty() || rest.starts_with('/')) else {
        refuse(stream, 404, "bench_no_route", "bench proxy: the path must start with /r/<run>/v1", req.keep_alive).await?;
        return Ok(req.keep_alive);
    };
    let rest = rest.trim_start_matches('/');
    let Some(meter) = inner.runs.lock().unwrap().get(run).cloned() else {
        refuse(stream, 404, "bench_run_over", "bench proxy: this run is over", req.keep_alive).await?;
        return Ok(req.keep_alive);
    };
    let model_call = req.method == "POST" && matches!(rest, "chat/completions" | "responses");

    let mut body = req.body.clone();
    let mut call = Call { at_ms: meter.opened.elapsed().as_millis() as u64, ..Call::default() };
    let mut injected = false;
    if model_call && let Ok(mut v) = serde_json::from_slice::<Value>(&body) {
        let length = |key: &str| v.get(key).and_then(Value::as_array).map(Vec::len);
        call.model = v.get("model").and_then(Value::as_str).unwrap_or("").to_string();
        call.stream = v.get("stream").and_then(Value::as_bool).unwrap_or(false);
        call.messages = length("messages").or(length("input")).unwrap_or(0);
        call.tools = length("tools").unwrap_or(0);
        if rest == "chat/completions" && ask_for_usage(&mut v) {
            body = serde_json::to_vec(&v).unwrap_or(body);
            injected = true;
        }
    }
    let over = {
        let mut state = meter.state.lock().unwrap();
        let (metered, in_flight) = &mut *state;
        if !model_call {
            metered.other_requests += 1;
            false
        } else if metered.turns + *in_flight >= meter.max_turns {
            metered.refused += 1;
            true
        } else {
            *in_flight += 1;
            metered.usage_injected += u32::from(injected);
            if !call.model.is_empty() && !metered.models.contains(&call.model) {
                metered.models.push(call.model.clone());
            }
            false
        }
    };
    if over {
        meter.capped.notify_one();
        let message = format!("bench: this run has used its {} model requests; no further request is answered", meter.max_turns);
        refuse(stream, 400, "bench_turn_limit", &message, req.keep_alive).await?;
        return Ok(req.keep_alive);
    }

    let url = format!("{}/{rest}{}{query}", inner.upstream, if query.is_empty() { "" } else { "?" });
    let started = Instant::now();
    let relayed = relay(inner, stream, &req, &url, body).await;
    if model_call {
        let mut state = meter.state.lock().unwrap();
        let (metered, in_flight) = &mut *state;
        *in_flight -= 1;
        call.ms = started.elapsed().as_millis() as u64;
        metered.model_ms += call.ms;
        match relayed.answer {
            Ok(usage) => {
                metered.turns += 1;
                (metered.last_error, metered.last_status, call.status) = (None, 200, 200);
                match usage {
                    Some(usage) => {
                        metered.add(&call.model, call.tools > 0, &usage);
                        (call.prompt_tokens, call.cached_tokens, call.output_tokens) = (usage.prompt, usage.cached, usage.output);
                    }
                    None => metered.unmetered += 1,
                }
            }
            Err((status, message)) => {
                metered.failed += 1;
                call.status = status;
                (metered.last_error, metered.last_status) = (Some(message), status);
            }
        }
        metered.calls.push(call);
    }
    Ok(relayed.reusable)
}

async fn serve(inner: &Inner, mut stream: TcpStream) -> std::io::Result<()> {
    let _ = stream.set_nodelay(true);
    let mut buf = Vec::new();
    while let Some(req) = read_request(&mut stream, &mut buf).await? {
        if !handle(inner, &mut stream, req).await? {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
/// A stand-in provider for the tests of the proxy and of the external runs.
pub mod fake {
    use super::*;

    pub const CHAT: &str = r#"{"id":"c1","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1200,"completion_tokens":50,"total_tokens":1250,"prompt_tokens_details":{"cached_tokens":1000},"completion_tokens_details":{"reasoning_tokens":20}}}"#;
    pub const STREAM: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":null}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":900,\"completion_tokens\":30,\"prompt_cache_hit_tokens\":800}}\n\ndata: [DONE]\n\n";
    const STREAM_WITHOUT_USAGE: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

    /// What the fake provider received: (method, target, authorization, body).
    pub type Seen = Arc<Mutex<Vec<(String, String, String, Value)>>>;

    /// A provider that answers chat completions the way the body asks: a stream with usage only
    /// when it was asked for, an error for the model `broken`.
    pub async fn provider() -> (String, Seen) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
        let seen: Seen = Arc::default();
        let log = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    while let Ok(Some(req)) = read_request(&mut stream, &mut buf).await {
                        let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                        let auth = req.headers.iter().find(|(n, _)| n == "authorization").map(|(_, v)| v.clone()).unwrap_or_default();
                        log.lock().unwrap().push((req.method.clone(), req.target.clone(), auth, body.clone()));
                        let (status, kind, text) = if req.method == "GET" {
                            (200, "application/json", r#"{"data":[]}"#)
                        } else if body["model"] == "broken" {
                            (503, "application/json", r#"{"error":{"message":"overloaded"}}"#)
                        } else if body["stream"] == true && body["stream_options"]["include_usage"] == true {
                            (200, "text/event-stream", STREAM)
                        } else if body["stream"] == true {
                            (200, "text/event-stream", STREAM_WITHOUT_USAGE)
                        } else {
                            (200, "application/json", CHAT)
                        };
                        let head = format!("HTTP/1.1 {status} {}\r\ncontent-type: {kind}\r\ncontent-length: {}\r\n\r\n", reason(status), text.len());
                        // In two pieces, so that an event is split across reads.
                        let (first, second) = text.split_at(text.len() / 2);
                        stream.write_all(head.as_bytes()).await.unwrap();
                        stream.write_all(first.as_bytes()).await.unwrap();
                        stream.flush().await.unwrap();
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        stream.write_all(second.as_bytes()).await.unwrap();
                    }
                });
            }
        });
        (base, seen)
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{CHAT, STREAM, provider as fake_provider};
    use super::*;

    fn client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    async fn post(base: &str, path: &str, body: Value) -> (u16, String) {
        let resp = client().post(format!("{base}/{path}")).bearer_auth("dummy").json(&body).send().await.unwrap();
        (resp.status().as_u16(), resp.text().await.unwrap())
    }

    #[tokio::test]
    async fn counts_the_usage_of_plain_and_streamed_answers_and_passes_them_through_unchanged() {
        let (upstream, seen) = fake_provider().await;
        let proxy = Proxy::start(&upstream, "real-key").await.unwrap();
        let (base, _meter) = proxy.open("run-a", 10);

        let (status, text) = post(&base, "chat/completions", json!({"model": "deepseek-chat", "messages": []})).await;
        assert_eq!((status, text.as_str()), (200, CHAT));
        let (status, text) = post(&base, "chat/completions", json!({"model": "deepseek-chat", "messages": [], "stream": true, "stream_options": {"include_usage": true}})).await;
        assert_eq!((status, text.as_str()), (200, STREAM));
        // A harness that streams without asking for usage gets it asked for on its behalf.
        let (status, text) = post(&base, "chat/completions", json!({"model": "deepseek-chat", "messages": [], "stream": true})).await;
        assert_eq!((status, text.as_str()), (200, STREAM));
        let listing = client().get(format!("{base}/models?limit=5")).send().await.unwrap();
        assert_eq!(listing.status().as_u16(), 200);

        let m = proxy.close("run-a");
        assert_eq!((m.turns, m.unmetered, m.usage_injected, m.failed, m.refused, m.other_requests), (3, 0, 1, 0, 0, 1));
        assert_eq!((m.prompt_tokens, m.cached_tokens, m.output_tokens, m.reasoning_tokens), (1200 + 900 + 900, 1000 + 800 + 800, 50 + 30 + 30, 20));
        assert_eq!((m.first_prompt_tokens, m.context_tokens), (1200, 900));
        assert_eq!(m.models, vec!["deepseek-chat"]);
        assert_eq!(m.calls.iter().map(|c| (c.status, c.stream, c.messages, c.prompt_tokens, c.output_tokens)).collect::<Vec<_>>(), [(200, false, 0, 1200, 50), (200, true, 0, 900, 30), (200, true, 0, 900, 30)]);
        assert!(m.priced && m.cost > 0.0 && m.cost_uncached > m.cost, "{m:?}");
        let prices = prices_for("deepseek-chat").unwrap();
        let listed = prices.cost(&Usage { prompt: 3000, cached: 2600, output: 110, reasoning: 20 });
        let peak = m.cost / listed;
        assert!((peak - 1.0).abs() < 1e-9 || (peak - 2.0).abs() < 1e-9, "cost {} against {listed} at list price", m.cost);

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 4);
        assert!(seen.iter().all(|(_, _, auth, _)| auth == "Bearer real-key"), "the real key replaces the harness's: {seen:?}");
        assert_eq!(seen[0].1, "/api/v1/chat/completions");
        assert_eq!(seen[2].3["stream_options"]["include_usage"], true);
        assert_eq!(seen[3].1, "/api/v1/models?limit=5");
    }

    #[tokio::test]
    async fn refuses_requests_beyond_the_turn_limit_and_keeps_runs_apart() {
        let (upstream, seen) = fake_provider().await;
        let proxy = Proxy::start(&upstream, "real-key").await.unwrap();
        let (a, meter) = proxy.open("a", 2);
        let (b, _) = proxy.open("b", 2);
        let ask = json!({"model": "glm-5.3", "messages": []});
        for base in [&a, &a, &b] {
            assert_eq!(post(base, "chat/completions", ask.clone()).await.0, 200);
        }
        let (status, text) = post(&a, "chat/completions", ask.clone()).await;
        assert_eq!(status, 400);
        assert!(text.contains("bench_turn_limit") && text.contains("used its 2 model requests"), "{text}");
        tokio::time::timeout(Duration::from_secs(1), meter.capped.notified()).await.expect("the runner is told");

        let (a, b) = (proxy.close("a"), proxy.close("b"));
        assert_eq!((a.turns, a.refused, a.prompt_tokens), (2, 1, 2400));
        assert_eq!((b.turns, b.refused, b.prompt_tokens), (1, 0, 1200));
        assert_eq!(seen.lock().unwrap().len(), 3, "a refused request never reaches the provider");
        // A closed run and a path outside the scheme are not forwarded either.
        assert_eq!(post(&format!("http://127.0.0.1:{}/r/a/v1", proxy.port), "chat/completions", ask.clone()).await.0, 404);
        assert_eq!(post(&format!("http://127.0.0.1:{}/v1", proxy.port), "chat/completions", ask).await.0, 404);
    }

    #[tokio::test]
    async fn a_failed_request_is_passed_on_and_recorded_but_is_no_turn() {
        let (upstream, _) = fake_provider().await;
        let proxy = Proxy::start(&upstream, "real-key").await.unwrap();
        let (base, _) = proxy.open("r", 5);
        let (status, text) = post(&base, "chat/completions", json!({"model": "broken", "messages": []})).await;
        assert_eq!((status, text.contains("overloaded")), (503, true));
        let m = proxy.close("r");
        assert_eq!((m.turns, m.failed, m.last_status, m.calls[0].status), (0, 1, 503, 503));
        assert!(m.last_error.as_deref().is_some_and(|e| e.starts_with("provider returned HTTP 503") && e.contains("overloaded")), "{m:?}");

        let unreachable = Proxy::start("http://127.0.0.1:1/v1", "k").await.unwrap();
        let (base, _) = unreachable.open("r", 5);
        assert_eq!(post(&base, "chat/completions", json!({"model": "glm-5.3"})).await.0, 502);
        let m = unreachable.close("r");
        assert_eq!((m.turns, m.failed, m.last_status), (0, 1, 0));
        assert!(m.last_error.unwrap().starts_with("network error"));
    }

    #[test]
    fn usage_is_read_from_chat_completions_and_from_the_responses_api() {
        let mut scanner = Scanner::new(true);
        // Events split at arbitrary points; the last usage reported wins.
        for piece in ["data: {\"usage\":{\"prompt_tokens\":5,\"compl", "etion_tokens\":1}}\n\nda", "ta: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":70,\"input_tokens_details\":{\"cached_tokens\":64},\"output_tokens\":9,\"output_tokens_details\":{\"reasoning_tokens\":4}}}}\n\n"] {
            scanner.feed(piece.as_bytes());
        }
        assert_eq!(scanner.finish(), Some(Usage { prompt: 70, cached: 64, output: 9, reasoning: 4 }));

        let mut plain = Scanner::new(false);
        plain.feed(CHAT.as_bytes());
        assert_eq!(plain.finish(), Some(Usage { prompt: 1200, cached: 1000, output: 50, reasoning: 20 }));
        assert_eq!(Scanner::new(false).finish(), None);
        let mut mislabelled = Scanner::new(false);
        mislabelled.feed(STREAM.as_bytes());
        assert_eq!(mislabelled.finish(), Some(Usage { prompt: 900, cached: 800, output: 30, reasoning: 0 }));
    }

    #[test]
    fn only_streams_that_do_not_report_usage_are_changed() {
        let mut plain = json!({"model": "m"});
        assert!(!ask_for_usage(&mut plain));
        let mut asked = json!({"stream": true, "stream_options": {"include_usage": true}});
        assert!(!ask_for_usage(&mut asked));
        let mut silent = json!({"stream": true, "stream_options": {"include_usage": false, "x": 1}});
        assert!(ask_for_usage(&mut silent));
        assert_eq!(silent, json!({"stream": true, "stream_options": {"include_usage": true, "x": 1}}));
    }
}
