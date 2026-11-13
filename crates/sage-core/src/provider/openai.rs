//! OpenAI-compatible chat-completions client with streaming, retries and vendor dialects.
//!
//! The wire shapes handled here were verified against live responses from DeepSeek V4.1 Flash and
//! GLM-5.3 (via an OpenRouter-style gateway); the native DeepSeek and Z.ai dialects follow their
//! published API contracts. So do the dialects of OpenAI, Anthropic, Gemini, xAI, Mistral, Groq,
//! Together and Fireworks, read from their documentation on 11 November 2026: none has been called.

use super::{ChatRequest, ChatResponse, Delta, DeltaSink, Effort, Message, ProviderError, Role, ToolCall, Usage, is_looping};
use crate::text::est_tokens;
use futures_util::StreamExt;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// How a vendor spells "think this hard" and "here is what I thought before".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    /// OpenRouter and gateways shaped like it: `reasoning: {effort}`, reasoning echoed as `reasoning`.
    OpenRouter,
    /// api.deepseek.com: `thinking` + `reasoning_effort`, reasoning echoed as `reasoning_content`
    /// (mandatory in tool loops: the API answers 400 without it).
    DeepSeek,
    /// api.z.ai: like DeepSeek plus `clear_thinking: false` so earlier reasoning stays in the prompt.
    Zai,
    /// logfare.ai: shaped like OpenRouter, but GLM there follows only a top-level `reasoning_effort`
    /// of `low`, `high` or `max` and thinks at `max` for anything else (measured 11 November 2026).
    Logfare,
    /// Plain OpenAI-style `reasoning_effort` and `max_tokens`: what self-hosted servers take.
    OpenAi,
    /// api.openai.com itself: `max_completion_tokens` (its reasoning models refuse `max_tokens`),
    /// and no reasoning text in either direction.
    OpenAiPlatform,
    /// Anthropic's OpenAI-compatible endpoint: it ignores `reasoning_effort`, returns no thinking
    /// and does not cache prompts.
    Anthropic,
    /// Gemini's OpenAI-compatible endpoint: efforts up to `high`, thinking that cannot be switched
    /// off, and on every tool call a thought signature that has to come back with it.
    Gemini,
    /// api.x.ai: `reasoning_effort` up to `xhigh`.
    Xai,
    /// api.mistral.ai: no `stream_options`, efforts from `none` to `xhigh`, and thinking streamed
    /// as chunks of `content`.
    Mistral,
    /// api.groq.com: `max_completion_tokens`, an effort on some models only, reasoning as `reasoning`.
    Groq,
    /// api.together.ai: an effort on the gpt-oss models only.
    Together,
    /// api.fireworks.ai: efforts up to `high`, reasoning as `reasoning_content`.
    Fireworks,
    /// api.moonshot.ai: `max_completion_tokens`, and Kimi K3 models that always think, at `low`,
    /// `high` or `max`, and take their `reasoning_content` back.
    Moonshot,
}

impl Dialect {
    pub fn parse(s: &str) -> Option<Dialect> {
        match s.trim().to_ascii_lowercase().as_str() {
            "openrouter" => Some(Dialect::OpenRouter),
            "logfare" => Some(Dialect::Logfare),
            "deepseek" => Some(Dialect::DeepSeek),
            "zai" | "z.ai" | "glm" => Some(Dialect::Zai),
            "openai" => Some(Dialect::OpenAi),
            "openai-platform" => Some(Dialect::OpenAiPlatform),
            "anthropic" => Some(Dialect::Anthropic),
            "gemini" | "google" => Some(Dialect::Gemini),
            "xai" | "grok" => Some(Dialect::Xai),
            "mistral" => Some(Dialect::Mistral),
            "groq" => Some(Dialect::Groq),
            "together" => Some(Dialect::Together),
            "fireworks" => Some(Dialect::Fireworks),
            "moonshot" | "kimi" => Some(Dialect::Moonshot),
            _ => None,
        }
    }

    /// The field of an assistant message that earlier reasoning goes back in, where the vendor
    /// takes it back at all.
    fn reasoning_key(&self) -> Option<&'static str> {
        match self {
            Dialect::OpenRouter | Dialect::Logfare | Dialect::Groq | Dialect::Together => Some("reasoning"),
            Dialect::DeepSeek | Dialect::Zai | Dialect::OpenAi | Dialect::Xai | Dialect::Fireworks | Dialect::Moonshot => Some("reasoning_content"),
            Dialect::OpenAiPlatform | Dialect::Anthropic | Dialect::Gemini | Dialect::Mistral => None,
        }
    }
}

/// `reasoning_effort` in a vendor's words: `off` is what switches thinking off there (nothing where
/// it cannot be), `max` the most it takes.
fn effort_word(effort: Effort, off: Option<&'static str>, max: &'static str) -> Option<&'static str> {
    match effort {
        Effort::Off => off,
        Effort::Low => Some("low"),
        Effort::High => Some("high"),
        Effort::Max => Some(max),
    }
}

/// What tells one tool call from another when its vendor's extras are looked up again.
fn extra_key(call: &ToolCall) -> String {
    format!("{}\u{0}{}\u{0}{}", call.id, call.name, call.arguments)
}

/// The text and the thinking of a `content` that came as chunks rather than as a string:
/// `[{type: "thinking", thinking: [{type: "text", text}]}, {type: "text", text}]`.
fn content_chunks(content: Option<&Value>) -> (Option<String>, Option<String>) {
    let Some(chunks) = content.and_then(Value::as_array) else { return (None, None) };
    let text_of = |chunk: &Value| chunk.get("text").and_then(Value::as_str).unwrap_or("").to_string();
    let (mut text, mut thought) = (String::new(), String::new());
    for chunk in chunks {
        match chunk.get("type").and_then(Value::as_str) {
            Some("thinking") => thought.extend(chunk.get("thinking").and_then(Value::as_array).into_iter().flatten().map(text_of)),
            _ => text.push_str(&text_of(chunk)),
        }
    }
    (Some(text), Some(thought))
}

pub struct OpenAiProvider {
    pub base_url: String,
    pub model: String,
    pub dialect: Dialect,
    pub max_retries: u32,
    /// While set, a failure that can be retried is retried past `max_retries`, for as long as it
    /// takes: a rate limit ends when the provider says so, not after a count. Shared, so that it
    /// can be switched while a request is waiting for its next attempt.
    pub keep_retrying: Arc<AtomicBool>,
    pub idle_timeout: Duration,
    api_key: String,
    client: reqwest::Client,
    /// What a vendor attached to a tool call beyond the standard fields, to send back with it.
    extras: Mutex<HashMap<String, Value>>,
}

const MAX_BACKOFF_SECS: u64 = 60;

impl OpenAiProvider {
    pub fn new(base_url: &str, api_key: &str, model: &str, dialect: Dialect) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("sage/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(20))
            .tcp_keepalive(Duration::from_secs(30))
            .build()
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            dialect,
            max_retries: 8,
            keep_retrying: Arc::default(),
            idle_timeout: Duration::from_secs(300),
            api_key: api_key.to_string(),
            client,
            extras: Mutex::default(),
        })
    }

    /// Tool calls in wire form with Gemini's thought signatures put back. A reply none of whose
    /// calls has a known one gets, on its first call, the marker Gemini documents for skipping the
    /// check: without either it answers 400. Of parallel calls only the first carries a signature.
    fn signed(&self, calls: &[ToolCall], mut wire: Vec<Value>) -> Vec<Value> {
        if self.dialect != Dialect::Gemini {
            return wire;
        }
        let extras = self.extras.lock().unwrap();
        let known: Vec<Option<&Value>> = calls.iter().map(|call| extras.get(&extra_key(call))).collect();
        let none_known = known.iter().all(Option::is_none);
        for (i, (call, extra)) in wire.iter_mut().zip(known).enumerate() {
            match extra {
                Some(extra) => call["extra_content"] = extra.clone(),
                None if none_known && i == 0 => call["extra_content"] = json!({"google": {"thought_signature": "skip_thought_signature_validator"}}),
                None => {}
            }
        }
        wire
    }

    /// Messages in wire form. Construction is deterministic so that an append-only history always
    /// serialises to a byte-identical prefix: that prefix is what the provider's cache is keyed on.
    pub fn wire_messages(&self, messages: &[Message]) -> Value {
        Value::Array(
            messages
                .iter()
                .map(|m| {
                    let mut o = Map::new();
                    o.insert("role".into(), json!(m.role.as_str()));
                    match m.role {
                        Role::Assistant => {
                            o.insert("content".into(), json!(m.content));
                            if let (Some(key), Some(r)) = (self.dialect.reasoning_key(), m.reasoning.as_deref().filter(|r| !r.is_empty())) {
                                o.insert(key.into(), json!(r));
                            }
                            if !m.tool_calls.is_empty() {
                                let calls: Vec<Value> = m
                                    .tool_calls
                                    .iter()
                                    .map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.arguments}}))
                                    .collect();
                                o.insert("tool_calls".into(), Value::Array(self.signed(&m.tool_calls, calls)));
                            }
                        }
                        Role::Tool => {
                            o.insert("tool_call_id".into(), json!(m.tool_call_id.as_deref().unwrap_or("")));
                            o.insert("content".into(), json!(m.content));
                        }
                        Role::System | Role::User => {
                            o.insert("content".into(), json!(m.content));
                        }
                    }
                    Value::Object(o)
                })
                .collect(),
        )
    }

    pub fn body(&self, req: &ChatRequest<'_>) -> Value {
        let mut b = Map::new();
        b.insert("model".into(), json!(self.model));
        b.insert("messages".into(), self.wire_messages(req.messages));
        if req.tools.as_array().is_some_and(|t| !t.is_empty()) {
            b.insert("tools".into(), req.tools.clone());
        }
        b.insert("stream".into(), json!(true));
        if self.dialect != Dialect::Mistral {
            b.insert("stream_options".into(), json!({"include_usage": true}));
        }
        let limit = if matches!(self.dialect, Dialect::OpenAiPlatform | Dialect::Groq | Dialect::Moonshot) { "max_completion_tokens" } else { "max_tokens" };
        b.insert(limit.into(), json!(req.max_tokens));
        let effort = req.effort;
        match self.dialect {
            Dialect::OpenRouter => {
                if effort == Effort::Off {
                    b.insert("reasoning".into(), json!({"enabled": false}));
                } else {
                    b.insert("reasoning".into(), json!({"effort": effort.as_str()}));
                    b.insert("include_reasoning".into(), json!(true));
                }
            }
            Dialect::Logfare => {
                // Both spellings: DeepSeek follows `reasoning`, GLM only `reasoning_effort`, which
                // has no "off": `low` is as little as it thinks.
                if effort == Effort::Off {
                    b.insert("reasoning".into(), json!({"enabled": false}));
                    b.insert("reasoning_effort".into(), json!("low"));
                } else {
                    b.insert("reasoning".into(), json!({"effort": effort.as_str()}));
                    b.insert("include_reasoning".into(), json!(true));
                    b.insert("reasoning_effort".into(), json!(effort.as_str()));
                }
            }
            Dialect::DeepSeek => {
                if effort == Effort::Off {
                    b.insert("thinking".into(), json!({"type": "disabled"}));
                } else {
                    b.insert("thinking".into(), json!({"type": "enabled"}));
                    b.insert("reasoning_effort".into(), json!(effort.as_str()));
                }
            }
            Dialect::Zai => {
                if effort == Effort::Off {
                    b.insert("thinking".into(), json!({"type": "disabled"}));
                } else {
                    b.insert("thinking".into(), json!({"type": "enabled", "clear_thinking": false}));
                    b.insert("reasoning_effort".into(), json!(effort.as_str()));
                }
            }
            Dialect::OpenAi | Dialect::OpenAiPlatform => {
                if effort != Effort::Off {
                    b.insert("reasoning_effort".into(), json!(effort.as_str()));
                }
            }
            Dialect::Anthropic | Dialect::Gemini | Dialect::Xai | Dialect::Mistral | Dialect::Groq | Dialect::Together | Dialect::Fireworks | Dialect::Moonshot => {
                let model = self.model.as_str();
                let word = match self.dialect {
                    Dialect::Gemini | Dialect::Fireworks => effort_word(effort, None, "high"),
                    Dialect::Xai => effort_word(effort, None, "xhigh"),
                    Dialect::Mistral => effort_word(effort, Some("none"), "xhigh"),
                    // Efforts are per model there: gpt-oss always thinks, Qwen can stop, Llama takes none.
                    Dialect::Groq if model.starts_with("qwen/") => effort_word(effort, Some("none"), "high"),
                    Dialect::Groq | Dialect::Together if model.contains("gpt-oss") => effort_word(effort, Some("low"), "high"),
                    // K3 cannot stop thinking; the K2 models before it take no effort at all.
                    Dialect::Moonshot if model.starts_with("kimi-k3") => effort_word(effort, Some("low"), "max"),
                    _ => None,
                };
                if let Some(word) = word {
                    b.insert("reasoning_effort".into(), json!(word));
                }
            }
        }
        Value::Object(b)
    }

    pub async fn chat(&self, req: ChatRequest<'_>, on_delta: DeltaSink<'_>, cancel: &CancellationToken) -> Result<ChatResponse, ProviderError> {
        let body = serde_json::to_vec(&self.body(&req)).map_err(|e| ProviderError::Protocol(e.to_string()))?;
        let fallback_prompt = est_tokens(std::str::from_utf8(&body).unwrap_or(""));
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            match self.attempt(&body, req.reasoning_cap, on_delta, cancel).await {
                Ok(mut r) => {
                    r.retries = attempt - 1;
                    if !r.usage_reported {
                        r.usage.prompt = fallback_prompt;
                    }
                    return Ok(r);
                }
                Err(e) if e.retryable() && (attempt <= self.max_retries || self.keep_retrying.load(Ordering::Relaxed)) => {
                    let wait_secs = match &e {
                        ProviderError::Http { retry_after_secs: Some(s), .. } => (*s).clamp(1, MAX_BACKOFF_SECS),
                        _ => (1u64 << attempt.min(6)).min(MAX_BACKOFF_SECS),
                    };
                    // Spread retries a little so parallel sessions do not stampede a recovering provider.
                    let jitter_ms = crate::util::fnv64(&crate::util::now_ms().to_le_bytes()) % 750;
                    let wait_ms = wait_secs * 1000 + jitter_ms;
                    on_delta(Delta::Retry { attempt, wait_ms, reason: e.to_string() });
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(wait_ms)) => {}
                        _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn attempt(&self, body: &[u8], reasoning_cap: usize, on_delta: DeltaSink<'_>, cancel: &CancellationToken) -> Result<ChatResponse, ProviderError> {
        let send = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .body(body.to_vec())
            .send();
        // An endpoint that takes the request and then says nothing is waited for no longer than one
        // that stops in the middle of its answer: without a limit here the run would wait for ever.
        let resp = tokio::select! {
            r = tokio::time::timeout(self.idle_timeout, send) => match r {
                Ok(r) => r.map_err(|e| ProviderError::Network(e.to_string()))?,
                Err(_) => return Err(ProviderError::Network(format!("no answer from provider for {}s", self.idle_timeout.as_secs()))),
            },
            _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
        };
        let status = resp.status();
        if !status.is_success() {
            let retry_after_secs = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
            let text = tokio::time::timeout(self.idle_timeout, resp.text()).await.ok().and_then(Result::ok).unwrap_or_default();
            return Err(ProviderError::Http { status: status.as_u16(), message: error_message(&text), retry_after_secs });
        }

        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
        let mut acc = Accum { reasoning_cap, ..Accum::default() };
        'read: loop {
            let next = tokio::select! {
                n = tokio::time::timeout(self.idle_timeout, stream.next()) => n,
                _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
            };
            let chunk = match next {
                Err(_) => return Err(ProviderError::Network(format!("no data from provider for {}s", self.idle_timeout.as_secs()))),
                Ok(None) => break,
                Ok(Some(Err(e))) => return Err(ProviderError::Network(e.to_string())),
                Ok(Some(Ok(bytes))) => bytes,
            };
            buf.extend_from_slice(&chunk);
            let mut start = 0;
            while let Some(nl) = buf[start..].iter().position(|b| *b == b'\n') {
                let line_end = start + nl;
                let done = acc.feed_line(&buf[start..line_end], on_delta)?;
                start = line_end + 1;
                if done {
                    break 'read;
                }
            }
            buf.drain(..start);
        }
        if !buf.is_empty() {
            acc.feed_line(&buf, on_delta)?;
        }
        let (response, extras) = acc.finish_with_extras()?;
        if !extras.is_empty() {
            self.extras.lock().unwrap().extend(extras);
        }
        Ok(response)
    }
}

fn error_message(body: &str) -> String {
    let from_json = serde_json::from_str::<Value>(body).ok().and_then(|v| {
        let e = v.get("error")?;
        e.get("message").and_then(Value::as_str).map(String::from).or_else(|| e.as_str().map(String::from))
    });
    crate::util::truncate_chars(from_json.as_deref().unwrap_or(body.trim()), 400)
}

fn u64_at(v: &Value, path: &[&str]) -> Option<u64> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_u64()
}

pub(crate) fn parse_usage(u: &Value) -> Usage {
    Usage {
        prompt: u64_at(u, &["prompt_tokens"]).or_else(|| u64_at(u, &["input_tokens"])).unwrap_or(0),
        cached: u64_at(u, &["prompt_tokens_details", "cached_tokens"])
            .or_else(|| u64_at(u, &["prompt_cache_hit_tokens"]))
            .or_else(|| u64_at(u, &["input_tokens_details", "cached_tokens"]))
            .or_else(|| u64_at(u, &["cache_read_input_tokens"]))
            .unwrap_or(0),
        output: u64_at(u, &["completion_tokens"]).or_else(|| u64_at(u, &["output_tokens"])).unwrap_or(0),
        reasoning: u64_at(u, &["completion_tokens_details", "reasoning_tokens"]).or_else(|| u64_at(u, &["reasoning_tokens"])).unwrap_or(0),
    }
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
    /// `extra_content`: where Gemini puts a call's thought signature.
    extra: Option<Value>,
}

/// Accumulates one streamed response.
#[derive(Default)]
pub(crate) struct Accum {
    content: String,
    reasoning: String,
    calls: Vec<PartialCall>,
    usage: Option<Usage>,
    finish: String,
    saw_event: bool,
    raw: String,
    unchecked: usize,
    /// Characters of reasoning allowed before anything else has come; 0 stands for `REASONING_CAP`.
    reasoning_cap: usize,
}

const LOOP_CHECK_EVERY: usize = 600;
/// Reasoning allowed in one reply before anything else has come (about 14,000 tokens). Past it the
/// reply would rather hit the output limit than act, having cost minutes.
const REASONING_CAP: usize = 56_000;

impl Accum {
    /// Feeds one line of the response body. Returns true once the stream signalled completion.
    pub(crate) fn feed_line(&mut self, line: &[u8], on_delta: DeltaSink<'_>) -> Result<bool, ProviderError> {
        let line = String::from_utf8_lossy(line);
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            return Ok(false);
        }
        let Some(payload) = line.strip_prefix("data:") else {
            // Not SSE. Either an `event:` field (ignored) or a plain JSON body from a server that
            // ignored `stream: true`; keep it so `finish` can try to read it as one.
            if !line.starts_with("event:") && !line.starts_with("id:") && !line.starts_with("retry:") && self.raw.len() < 4 * 1024 * 1024 {
                self.raw.push_str(line);
                self.raw.push('\n');
            }
            return Ok(false);
        };
        let payload = payload.trim();
        if payload == "[DONE]" {
            return Ok(true);
        }
        let v: Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        self.saw_event = true;
        self.apply_chunk(&v, on_delta)?;
        Ok(false)
    }

    fn apply_chunk(&mut self, v: &Value, on_delta: DeltaSink<'_>) -> Result<(), ProviderError> {
        if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
            let status = err.get("code").and_then(Value::as_u64).filter(|c| (400..600).contains(c)).unwrap_or(502) as u16;
            let message = err.get("message").and_then(Value::as_str).map(String::from).unwrap_or_else(|| err.to_string());
            return Err(ProviderError::Http { status, message: crate::util::truncate_chars(&message, 400), retry_after_secs: None });
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            self.usage = Some(parse_usage(u));
        }
        let Some(choice) = v.get("choices").and_then(|c| c.get(0)) else { return Ok(()) };
        if let Some(f) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish = f.to_string();
        }
        let Some(delta) = choice.get("delta").or_else(|| choice.get("message")) else { return Ok(()) };

        let (chunk_text, chunk_thought) = content_chunks(delta.get("content"));
        let reasoning = delta.get("reasoning_content").and_then(Value::as_str).or_else(|| delta.get("reasoning").and_then(Value::as_str)).or(chunk_thought.as_deref());
        if let Some(r) = reasoning.filter(|r| !r.is_empty()) {
            self.reasoning.push_str(r);
            self.unchecked += r.len();
            on_delta(Delta::Reasoning(r.to_string()));
        }
        if let Some(c) = delta.get("content").and_then(Value::as_str).or(chunk_text.as_deref()).filter(|c| !c.is_empty()) {
            self.content.push_str(c);
            self.unchecked += c.len();
            on_delta(Delta::Content(c.to_string()));
        }
        if self.unchecked >= LOOP_CHECK_EVERY {
            self.unchecked = 0;
            if is_looping(&self.reasoning) || is_looping(&self.content) {
                return Err(ProviderError::Degenerate);
            }
            let cap = if self.reasoning_cap == 0 { REASONING_CAP } else { self.reasoning_cap };
            if self.reasoning.len() > cap && self.content.is_empty() && self.calls.is_empty() {
                return Err(ProviderError::Overthinking(self.reasoning.len()));
            }
        }

        if let Some(tcs) = delta.get("tool_calls").and_then(Value::as_array) {
            for (pos, tc) in tcs.iter().enumerate() {
                let id = tc.get("id").and_then(Value::as_str).unwrap_or("");
                let func = tc.get("function");
                let name = func.and_then(|f| f.get("name")).and_then(Value::as_str).unwrap_or("");
                let args = match func.and_then(|f| f.get("arguments")) {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Null) | None => String::new(),
                    Some(other) => other.to_string(),
                };
                let slot = match tc.get("index").and_then(Value::as_u64) {
                    Some(i) => i as usize,
                    None if !id.is_empty() => self.calls.iter().position(|c| c.id == id).unwrap_or(self.calls.len()),
                    None => self.calls.len().saturating_sub(1).max(pos),
                };
                if slot > 256 {
                    return Err(ProviderError::Protocol("tool call index out of range".into()));
                }
                while self.calls.len() <= slot {
                    self.calls.push(PartialCall::default());
                }
                let call = &mut self.calls[slot];
                if !id.is_empty() {
                    call.id = id.to_string();
                }
                if !name.is_empty() && call.name != name {
                    call.name.push_str(name);
                }
                call.arguments.push_str(&args);
                if let Some(extra) = tc.get("extra_content").filter(|extra| !extra.is_null()) {
                    call.extra = Some(extra.clone());
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn finish(self) -> Result<ChatResponse, ProviderError> {
        self.finish_with_extras().map(|(response, _)| response)
    }

    /// The response, and with it what the vendor attached to its tool calls, by `extra_key`.
    fn finish_with_extras(mut self) -> Result<(ChatResponse, Vec<(String, Value)>), ProviderError> {
        if !self.saw_event && !self.raw.trim().is_empty() {
            let v: Value = serde_json::from_str(self.raw.trim()).map_err(|_| ProviderError::Protocol(crate::util::truncate_chars(self.raw.trim(), 200)))?;
            let mut sink = |_d: Delta| {};
            self.apply_chunk(&v, &mut sink)?;
        }
        let mut extras = Vec::new();
        let calls: Vec<ToolCall> = self
            .calls
            .into_iter()
            .filter(|c| !c.name.is_empty() || !c.arguments.trim().is_empty())
            .enumerate()
            .map(|(i, c)| {
                let call = ToolCall { id: if c.id.is_empty() { format!("call_{i}") } else { c.id }, name: c.name, arguments: c.arguments };
                if let Some(extra) = c.extra {
                    extras.push((extra_key(&call), extra));
                }
                call
            })
            .collect();
        if self.content.is_empty() && self.reasoning.is_empty() && calls.is_empty() {
            return Err(ProviderError::Protocol("empty response".into()));
        }
        let usage_reported = self.usage.is_some();
        let usage = self.usage.unwrap_or_else(|| {
            let reasoning = est_tokens(&self.reasoning);
            let args: u64 = calls.iter().map(|c| est_tokens(&c.arguments) + 8).sum();
            Usage { prompt: 0, cached: 0, output: reasoning + est_tokens(&self.content) + args, reasoning }
        });
        Ok((ChatResponse { content: self.content, reasoning: self.reasoning, calls, usage, usage_reported, finish: self.finish, retries: 0 }, extras))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(dialect: Dialect) -> OpenAiProvider {
        OpenAiProvider::new("https://example.invalid/v1/", "k", "m", dialect).unwrap()
    }

    fn feed(lines: &[&str]) -> Result<(ChatResponse, Vec<String>), ProviderError> {
        let mut acc = Accum::default();
        let mut seen = Vec::new();
        let mut sink = |d: Delta| match d {
            Delta::Reasoning(t) => seen.push(format!("r:{t}")),
            Delta::Content(t) => seen.push(format!("c:{t}")),
            Delta::Retry { .. } => {}
        };
        for l in lines {
            if acc.feed_line(l.as_bytes(), &mut sink)? {
                break;
            }
        }
        Ok((acc.finish()?, seen))
    }

    /// A local endpoint that answers the first `failures` requests with a rate limit, then a reply.
    async fn rate_limited_endpoint(failures: usize) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut served = 0;
            while let Ok((mut socket, _)) = listener.accept().await {
                // The whole request is read before answering, or the answer may be lost with it.
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                    if let Some(head) = text.find("\r\n\r\n") {
                        let length = text[..head].lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                        if request.len() >= head + 4 + length {
                            break;
                        }
                    }
                    match socket.read(&mut chunk).await {
                        Ok(n) if n > 0 => request.extend_from_slice(&chunk[..n]),
                        _ => break,
                    }
                }
                let (status, extra, body) = if served < failures {
                    ("429 Too Many Requests", "retry-after: 1\r\n", r#"{"error":{"message":"rate limited"}}"#)
                } else {
                    ("200 OK", "", r#"{"choices":[{"message":{"role":"assistant","content":"done"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#)
                };
                served += 1;
                let answer = format!("HTTP/1.1 {status}\r\ncontent-type: application/json\r\n{extra}content-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                let _ = socket.write_all(answer.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        format!("http://{address}/v1")
    }

    #[tokio::test]
    async fn an_endpoint_that_takes_the_request_and_never_answers_is_given_up_on() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            // Connections are kept open and never answered.
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        let mut p = OpenAiProvider::new(&format!("http://{address}/v1"), "k", "m", Dialect::OpenAi).unwrap();
        p.max_retries = 0;
        p.idle_timeout = Duration::from_millis(200);
        let messages = [Message::user("hi")];
        let tools = json!([]);
        let request = ChatRequest { messages: &messages, tools: &tools, effort: Effort::High, max_tokens: 64, reasoning_cap: 0 };
        let cancel = CancellationToken::new();
        let answer = tokio::time::timeout(Duration::from_secs(20), p.chat(request, &mut |_| {}, &cancel)).await.expect("the request must not wait for ever");
        assert!(matches!(answer, Err(ProviderError::Network(_))), "{:?}", answer.err().map(|e| e.to_string()));
    }

    #[tokio::test]
    async fn gives_up_at_the_limit_unless_told_to_keep_retrying() {
        let mut p = OpenAiProvider::new(&rate_limited_endpoint(3).await, "k", "m", Dialect::OpenAi).unwrap();
        p.max_retries = 0;
        let messages = [Message::user("hi")];
        let tools = json!([]);
        let ask = || ChatRequest { messages: &messages, tools: &tools, effort: Effort::High, max_tokens: 64, reasoning_cap: 0 };
        let cancel = CancellationToken::new();

        let gave_up = p.chat(ask(), &mut |_| {}, &cancel).await.unwrap_err();
        assert!(matches!(gave_up, ProviderError::Http { status: 429, .. }), "{gave_up}");

        // Two rate limits are left: both are waited out, past a limit of no retries at all.
        p.keep_retrying.store(true, Ordering::Relaxed);
        let mut attempts = Vec::new();
        let mut sink = |d: Delta| {
            if let Delta::Retry { attempt, .. } = d {
                attempts.push(attempt);
            }
        };
        let reply = p.chat(ask(), &mut sink, &cancel).await.unwrap();
        assert_eq!((reply.content.as_str(), reply.retries, attempts.as_slice()), ("done", 2, &[1, 2][..]));
    }

    #[test]
    fn parses_stream_as_seen_live_from_glm() {
        // Shapes copied from a real GLM-5.3 stream: reasoning deltas, whole tool calls per chunk,
        // then a usage-only chunk with empty choices.
        let (r, seen) = feed(&[
            r#"data: {"choices":[{"index":0,"delta":{"role":"assistant","reasoning":"The"},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"index":0,"delta":{"reasoning":" user wants"},"finish_reason":null}]}"#,
            ": keep-alive",
            r#"data: {"choices":[{"index":0,"delta":{"content":"","tool_calls":[{"index":0,"id":"a1","type":"function","function":{"name":"read","arguments":"{\"path\":\"a.txt\"}"}}],"reasoning":"."},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"index":0,"delta":{"content":"","tool_calls":[{"index":1,"id":"b2","type":"function","function":{"name":"shell","arguments":"{\"cmd\":\"ls\",\"expect\":{\"exit\":0}}"}}]},"finish_reason":"tool_calls"}]}"#,
            r#"data: {"choices":[],"usage":{"prompt_tokens":1610,"completion_tokens":104,"total_tokens":1714,"prompt_tokens_details":{"cached_tokens":1536},"completion_tokens_details":{"reasoning_tokens":63}}}"#,
            "data: [DONE]",
            r#"data: {"choices":[{"delta":{"content":"ignored after DONE"}}]}"#,
        ])
        .unwrap();
        assert_eq!(r.reasoning, "The user wants.");
        assert_eq!(seen, vec!["r:The", "r: user wants", "r:."]);
        assert_eq!(r.content, "");
        assert_eq!(r.calls.len(), 2);
        assert_eq!(r.calls[1], ToolCall { id: "b2".into(), name: "shell".into(), arguments: r#"{"cmd":"ls","expect":{"exit":0}}"#.into() });
        assert_eq!(r.usage, Usage { prompt: 1610, cached: 1536, output: 104, reasoning: 63 });
        assert!(r.usage_reported);
        assert_eq!(r.finish, "tool_calls");
    }

    #[test]
    fn assembles_fragmented_tool_arguments_and_deepseek_usage_fields() {
        let (r, _) = feed(&[
            r#"data: {"choices":[{"delta":{"reasoning_content":"hm"}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"edit","arguments":""}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a.py\"}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":900,"prompt_cache_hit_tokens":800,"prompt_cache_miss_tokens":100,"completion_tokens":40}}"#,
        ])
        .unwrap();
        assert_eq!(r.reasoning, "hm");
        assert_eq!(r.calls, vec![ToolCall { id: "c1".into(), name: "edit".into(), arguments: r#"{"path":"a.py"}"#.into() }]);
        assert_eq!(r.usage.cached, 800);
    }

    #[test]
    fn accepts_a_plain_json_body_when_the_server_ignores_streaming() {
        let (r, _) = feed(&[r#"{"choices":[{"message":{"role":"assistant","content":"done","reasoning":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#]).unwrap();
        assert_eq!((r.content.as_str(), r.reasoning.as_str(), r.finish.as_str()), ("done", "ok", "stop"));
    }

    #[test]
    fn surfaces_midstream_errors_and_empty_responses() {
        let err = feed(&[r#"data: {"error":{"message":"Provider overloaded","code":429}}"#]).unwrap_err();
        assert!(matches!(err, ProviderError::Http { status: 429, .. }) && err.retryable());
        assert!(matches!(feed(&["data: [DONE]"]).unwrap_err(), ProviderError::Protocol(_)));
    }

    #[test]
    fn cuts_a_looping_stream() {
        let chunk = format!(r#"data: {{"choices":[{{"delta":{{"reasoning":"{}"}}}}]}}"#, "I should re-check the file. ".repeat(30));
        let lines: Vec<&str> = vec![chunk.as_str(); 4];
        assert!(matches!(feed(&lines).unwrap_err(), ProviderError::Degenerate));

        // Thinking without end is cut too, unless an answer or a call has begun.
        let thought: String = (0..700).map(|i| format!("Step {i}: consider case {i}. ")).collect();
        let chunk = format!(r#"data: {{"choices":[{{"delta":{{"reasoning":"{thought}"}}}}]}}"#);
        assert!(matches!(feed(&[chunk.as_str(); 4]).unwrap_err(), ProviderError::Overthinking(n) if n > REASONING_CAP));
        // Where long thought is allowed, the same reply is let through.
        let mut patient = Accum { reasoning_cap: 8 * REASONING_CAP, ..Accum::default() };
        for _ in 0..4 {
            assert!(matches!(patient.feed_line(chunk.as_bytes(), &mut |_| {}), Ok(false)));
        }
        let answer = r#"data: {"choices":[{"delta":{"content":"Here is the answer."}}]}"#;
        assert!(feed(&[answer, chunk.as_str(), chunk.as_str(), chunk.as_str(), chunk.as_str(), "data: [DONE]"]).is_ok());
    }

    #[test]
    fn wire_prefix_is_byte_stable_when_history_only_grows() {
        let p = provider(Dialect::DeepSeek);
        let mut msgs = vec![Message::system("sys"), Message::user("task")];
        let first = p.wire_messages(&msgs).to_string();
        msgs.push(Message {
            role: Role::Assistant,
            content: String::new(),
            reasoning: Some("thinking…".into()),
            tool_calls: vec![ToolCall { id: "x".into(), name: "shell".into(), arguments: "{\"cmd\": \"ls\" }".into() }],
            tool_call_id: None,
        });
        msgs.push(Message::tool("x", "[ok] exit 0"));
        let second = p.wire_messages(&msgs).to_string();
        assert!(second.starts_with(&first[..first.len() - 1]), "earlier turns must serialise identically");
        // Arguments are echoed exactly as produced, odd spacing included.
        assert!(second.contains(r#""arguments":"{\"cmd\": \"ls\" }""#));
        assert!(second.contains(r#""reasoning_content":"thinking…""#));
    }

    #[test]
    fn gemini_signatures_go_back_with_their_calls_and_chunked_content_is_read() {
        let mut acc = Accum::default();
        let mut sink = |_d: Delta| {};
        let lines = [
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"g1","function":{"name":"read","arguments":"{\"path\":\"a\"}"},"extra_content":{"google":{"thought_signature":"SIG"}}},{"index":1,"id":"g2","function":{"name":"read","arguments":"{\"path\":\"b\"}"}}]}}]}"#,
            "data: [DONE]",
        ];
        for line in lines {
            acc.feed_line(line.as_bytes(), &mut sink).unwrap();
        }
        let (response, extras) = acc.finish_with_extras().unwrap();
        assert_eq!(extras.len(), 1, "of parallel calls only the first carries a signature");

        let p = provider(Dialect::Gemini);
        p.extras.lock().unwrap().extend(extras);
        let unknown = ToolCall { id: "x".into(), name: "shell".into(), arguments: "{}".into() };
        let reply = |calls: Vec<ToolCall>| Message { role: Role::Assistant, content: "".into(), reasoning: None, tool_calls: calls, tool_call_id: None };
        let wire = p.wire_messages(&[reply(response.calls.clone()), reply(vec![unknown.clone()])]);
        assert_eq!(wire[0]["tool_calls"][0]["extra_content"], json!({"google": {"thought_signature": "SIG"}}));
        assert!(wire[0]["tool_calls"][1].get("extra_content").is_none());
        assert_eq!(wire[1]["tool_calls"][0]["extra_content"]["google"]["thought_signature"], json!("skip_thought_signature_validator"));
        assert!(provider(Dialect::OpenAi).wire_messages(&[reply(vec![unknown])])[0]["tool_calls"][0].get("extra_content").is_none());

        // Mistral: thinking and text as chunks of `content`.
        let (r, _) = feed(&[
            r#"data: {"choices":[{"delta":{"content":[{"type":"thinking","thinking":[{"type":"text","text":"hm"}]},{"type":"text","text":"done"}]},"finish_reason":"stop"}]}"#,
            "data: [DONE]",
        ])
        .unwrap();
        assert_eq!((r.reasoning.as_str(), r.content.as_str()), ("hm", "done"));
    }

    #[test]
    fn dialects_spell_reasoning_their_own_way() {
        let tools = json!([]);
        let msgs = [Message::user("x")];
        let req = |effort| ChatRequest { messages: &msgs, tools: &tools, effort, max_tokens: 100, reasoning_cap: 0 };

        let b = provider(Dialect::OpenRouter).body(&req(Effort::High));
        assert_eq!(b["reasoning"], json!({"effort": "high"}));
        assert_eq!(b["include_reasoning"], json!(true));
        assert!(b.get("tools").is_none(), "an empty tools array is omitted");

        let b = provider(Dialect::Logfare).body(&req(Effort::High));
        assert_eq!((&b["reasoning"], &b["reasoning_effort"]), (&json!({"effort": "high"}), &json!("high")));
        let b = provider(Dialect::Logfare).body(&req(Effort::Off));
        assert_eq!((&b["reasoning"], &b["reasoning_effort"]), (&json!({"enabled": false}), &json!("low")));

        let b = provider(Dialect::DeepSeek).body(&req(Effort::Max));
        assert_eq!((&b["thinking"], &b["reasoning_effort"]), (&json!({"type": "enabled"}), &json!("max")));
        assert_eq!(provider(Dialect::DeepSeek).body(&req(Effort::Off))["thinking"], json!({"type": "disabled"}));

        let b = provider(Dialect::Zai).body(&req(Effort::Low));
        assert_eq!(b["thinking"], json!({"type": "enabled", "clear_thinking": false}));

        // The vendors nobody here has called: what their documentation says they take.
        let b = provider(Dialect::OpenAiPlatform).body(&req(Effort::Max));
        assert_eq!((&b["max_completion_tokens"], &b["reasoning_effort"], b.get("max_tokens")), (&json!(100), &json!("max"), None));
        let b = provider(Dialect::Anthropic).body(&req(Effort::High));
        assert_eq!((b.get("reasoning_effort"), &b["max_tokens"]), (None, &json!(100)));
        let b = provider(Dialect::Gemini).body(&req(Effort::Max));
        assert_eq!(b["reasoning_effort"], json!("high"));
        assert!(provider(Dialect::Gemini).body(&req(Effort::Off)).get("reasoning_effort").is_none(), "Gemini cannot stop thinking");
        assert_eq!(provider(Dialect::Xai).body(&req(Effort::Max))["reasoning_effort"], json!("xhigh"));
        let b = provider(Dialect::Mistral).body(&req(Effort::Off));
        assert_eq!((&b["reasoning_effort"], b.get("stream_options")), (&json!("none"), None));
        let groq = |model: &str, effort| OpenAiProvider::new("https://example.invalid", "k", model, Dialect::Groq).unwrap().body(&req(effort));
        assert_eq!((&groq("openai/gpt-oss-120b", Effort::Max)["reasoning_effort"], &groq("openai/gpt-oss-120b", Effort::Off)["reasoning_effort"]), (&json!("high"), &json!("low")));
        assert!(groq("llama-3.3-70b-versatile", Effort::High).get("reasoning_effort").is_none());
        assert_eq!(groq("llama-3.3-70b-versatile", Effort::High)["max_completion_tokens"], json!(100));
        assert!(provider(Dialect::Together).body(&req(Effort::High)).get("reasoning_effort").is_none());
        assert_eq!(provider(Dialect::Fireworks).body(&req(Effort::Max))["reasoning_effort"], json!("high"));
        let kimi = |model: &str, effort| OpenAiProvider::new("https://example.invalid", "k", model, Dialect::Moonshot).unwrap().body(&req(effort));
        assert_eq!((&kimi("kimi-k3.1-pro", Effort::Max)["reasoning_effort"], &kimi("kimi-k3", Effort::Off)["reasoning_effort"]), (&json!("max"), &json!("low")));
        assert_eq!((kimi("kimi-k2.6", Effort::High).get("reasoning_effort"), &kimi("kimi-k3", Effort::High)["max_completion_tokens"]), (None, &json!(100)));

        let said = [Message { role: Role::Assistant, content: "".into(), reasoning: Some("r".into()), tool_calls: vec![], tool_call_id: None }];
        assert!(provider(Dialect::OpenAiPlatform).wire_messages(&said)[0].get("reasoning_content").is_none(), "OpenAI refuses fields it does not know");
        assert_eq!(provider(Dialect::Groq).wire_messages(&said)[0]["reasoning"], json!("r"));
        assert_eq!(provider(Dialect::Moonshot).wire_messages(&said)[0]["reasoning_content"], json!("r"));

        let p = provider(Dialect::OpenRouter);
        assert_eq!(p.base_url, "https://example.invalid/v1");
        let wire = p.wire_messages(&[Message { role: Role::Assistant, content: "".into(), reasoning: Some("r".into()), tool_calls: vec![], tool_call_id: None }]);
        assert_eq!(wire[0]["reasoning"], json!("r"));
    }
}
