//! `presage`: run the harness from a terminal.

mod bench;

use presage_core::events::{CacheVerdict, Event, EventKind, FinishStatus, Stats};
use presage_core::harness::{Harness, SessionStatus, StartRequest};
use presage_core::tools::StepStatus;
use presage_core::util::{fmt_bytes, fmt_duration};
use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;

const HELP: &str = "\
presage — an expectation-driven agent harness

USAGE
  presage run [options] <task…>     work on a task in a directory, unattended
  presage demo                      play the scripted walkthrough (no key, no network)
  presage bench [options]           compare harness variants on built-in tasks (presage bench --help)
  presage skills [show <name>]      list skills, or print one
  presage prompt [options]          print the system prompt `run` would send with the same options
  presage sessions                  list sessions
  presage show <session-id>         replay a session's log
  presage providers                 list providers and whether a key is set
  presage key <provider>            store an API key (read from stdin)

OPTIONS for run
  -C, --cwd <dir>          workspace (default: current directory)
  -p, --provider <id>      logfare | deepseek | openrouter | … (`presage providers` lists them all)
  -m, --model <name>       model id as the provider spells it
  -e, --effort <level>     low | high | max | ratchet (default; starts high, rises on repeated surprises)
      --classic            conventional loop without scripts, predictions, watches or the finish gate (baseline)
      --without <name>     switch one mechanism off, e.g. auto-watch (repeatable; for ablation runs)
      --skill <name>       switch a skill on (repeatable, or comma-separated)
      --system-file <path> system prompt template for this run
      --no-project-instructions
                           leave the project's AGENTS.md out of the system prompt
      --max-turns <n>      model replies allowed (default 80)
      --time-budget <s>    seconds allowed
  -v, --verbose            also print the model's reasoning and every observation
      --json               print the final result as JSON on stdout
      --events             print every event as a JSON line on stdout

Keys are read from the environment (LOGFARE_API_KEY, DEEPSEEK_API_KEY, ZAI_API_KEY, OPENROUTER_API_KEY)
or from ~/.presage/config.json (written by `presage key`).";

struct Paint {
    on: bool,
}

impl Paint {
    fn wrap(&self, code: &str, s: &str) -> String {
        if self.on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }
    fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }
    fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }
    fn green(&self, s: &str) -> String {
        self.wrap("32", s)
    }
    fn red(&self, s: &str) -> String {
        self.wrap("31", s)
    }
    fn yellow(&self, s: &str) -> String {
        self.wrap("33", s)
    }
    fn cyan(&self, s: &str) -> String {
        self.wrap("36", s)
    }
}

fn k(n: u64) -> String {
    if n < 1_000 { n.to_string() } else if n < 100_000 { format!("{:.1}k", n as f64 / 1e3) } else if n < 1_000_000 { format!("{}k", n / 1_000) } else { format!("{:.2}M", n as f64 / 1e6) }
}

fn pct(part: u64, whole: u64) -> String {
    (part * 100 + whole / 2).checked_div(whole).map_or("–".into(), |p| format!("{p}%"))
}

fn indent(text: &str, prefix: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = lines.iter().take(max_lines).map(|l| format!("{prefix}{l}")).collect();
    if lines.len() > max_lines {
        out.push(format!("{prefix}… {} more lines", lines.len() - max_lines));
    }
    out.join("\n")
}

struct Printer {
    paint: Paint,
    verbose: bool,
    pending: Option<(String, String, Option<String>)>,
}

impl Printer {
    fn stats_block(&self, s: &Stats, simulated: bool) -> String {
        let p = &self.paint;
        let cost = if simulated {
            "simulated run: token counts are fabricated".to_string()
        } else if s.priced {
            format!("cost ${:.4} at list price", s.cost)
        } else {
            "no price table for this model".to_string()
        };
        let preds = if s.predictions > 0 { format!(" · predictions {}/{} held", s.predictions_held, s.predictions) } else { String::new() };
        let skipped = if s.skipped > 0 { format!(" ({} skipped)", s.skipped) } else { String::new() };
        [
            format!("turns {} · steps {}{skipped} · surprises {}{preds}", s.turns, s.steps, s.surprises),
            format!("tokens: in {} ({} cached) · out {} ({} thinking) · {cost}", k(s.prompt_tokens), pct(s.cached_tokens, s.prompt_tokens), k(s.output_tokens), k(s.reasoning_tokens)),
            format!(
                "time {} (model {}, tools {}) · output kept out of context {} · repairs {} · retries {} · cache misses {}",
                fmt_duration(s.elapsed_ms),
                fmt_duration(s.model_ms),
                fmt_duration(s.tool_ms),
                fmt_bytes(s.hidden_bytes),
                s.repairs,
                s.retries,
                s.cache_misses
            ),
        ]
        .iter()
        .map(|l| p.dim(l))
        .collect::<Vec<_>>()
        .join("\n")
    }

    fn print(&mut self, ev: &Event) {
        let p = &self.paint;
        match &ev.kind {
            EventKind::SessionStart { id, cwd, provider, model, effort, mode, simulated, skills, .. } => {
                let sim = if *simulated { p.yellow("  [simulated model]") } else { String::new() };
                let skills = if skills.is_empty() { String::new() } else { format!(" · skills: {}", skills.join(", ")) };
                eprintln!("{} {model} via {provider} · effort {effort} · {mode}{skills}{sim}\n{}", p.bold("presage"), p.dim(&format!("{cwd} · session {id}")));
            }
            EventKind::Env { text } if self.verbose => eprintln!("{}", p.dim(&indent(text, "  ", 12))),
            EventKind::Retry { attempt, wait_ms, reason, .. } => eprintln!("  {}", p.yellow(&format!("provider hiccup (attempt {attempt}): {reason} — retrying in {}", fmt_duration(*wait_ms)))),
            EventKind::Model { turn, content, reasoning, usage, ms, repairs, .. } => {
                let fixed = if *repairs > 0 { format!(" · {repairs} repaired") } else { String::new() };
                eprintln!(
                    "{} {}",
                    p.cyan(&format!("▸ {turn}")),
                    p.dim(&format!("{} · in {} ({} cached) · out {} ({} thinking){fixed}", fmt_duration(*ms), k(usage.prompt), pct(usage.cached, usage.prompt), k(usage.output), k(usage.reasoning)))
                );
                if self.verbose && !reasoning.trim().is_empty() {
                    eprintln!("{}", p.dim(&indent(reasoning.trim(), "    ~ ", 14)));
                }
                if !content.trim().is_empty() {
                    eprintln!("{}", indent(content.trim(), "    ", 40));
                }
            }
            EventKind::Cache { verdict: CacheVerdict::Bust, .. } => eprintln!("    {}", p.yellow("the start of the prompt changed (old output trimmed, or a tool added): the provider cache restarts from there")),
            EventKind::StepStart { tool, summary, expect, .. } => self.pending = Some((tool.clone(), summary.clone(), expect.clone())),
            EventKind::StepEnd { status, observation, ms, violations, .. } => {
                let (tool, summary, expect) = self.pending.take().unwrap_or_default();
                let mark = match status {
                    StepStatus::Ok => p.green("✓"),
                    StepStatus::Surprise => p.red("!"),
                    StepStatus::Error => p.red("✗"),
                    StepStatus::Skipped => p.dim("–"),
                    StepStatus::Running => p.yellow("…"),
                    StepStatus::Denied => p.red("⊘"),
                };
                let predicted = expect.map(|e| p.dim(&format!("  ⟨expect {e}⟩"))).unwrap_or_default();
                let took = if *ms >= 100 { p.dim(&format!("  {}", fmt_duration(*ms))) } else { String::new() };
                eprintln!("    {mark} {tool:<6} {summary}{predicted}{took}");
                match status {
                    StepStatus::Surprise | StepStatus::Error | StepStatus::Denied | StepStatus::Running => {
                        if !violations.is_empty() {
                            eprintln!("        {}", p.red(&violations.join("; ")));
                        }
                        eprintln!("{}", p.dim(&indent(observation, "        | ", if self.verbose { 60 } else { 12 })));
                    }
                    StepStatus::Ok if self.verbose => eprintln!("{}", p.dim(&indent(observation, "        | ", 30))),
                    _ => {}
                }
            }
            EventKind::Watch { detail, name, .. } => {
                let line = if detail.is_empty() { format!("watch {name}: still passing") } else { detail.lines().next().unwrap_or("").to_string() };
                eprintln!("    {} {line}", p.cyan("◦"));
            }
            EventKind::Note { text, .. } => eprintln!("    {} {}", p.yellow("◦"), p.yellow(text)),
            EventKind::Lesson { learned, text } => eprintln!("    {} {} {text}", p.cyan("◦"), if *learned { "learned:" } else { "recalled:" }),
            EventKind::Effort { from, to, reason, .. } => eprintln!("    {} effort {from} → {to} ({reason})", p.yellow("◦")),
            EventKind::Settings { model, effort, autonomy, .. } => {
                let changed: Vec<String> = [("model", model), ("effort", effort), ("autonomy", autonomy)].iter().filter_map(|(name, value)| value.as_ref().map(|value| format!("{name} {value}"))).collect();
                eprintln!("    {} now {}", p.yellow("◦"), changed.join(", "));
            }
            EventKind::Decay { messages, dropped_tokens, reason, .. } => eprintln!("    {} trimmed {messages} old observations (~{} tokens): {reason}", p.yellow("◦"), k(*dropped_tokens)),
            EventKind::Approval { tool, summary, reason, .. } => eprintln!("    {} approval needed for {tool} {summary} ({reason})", p.yellow("?")),
            EventKind::Error { message } => eprintln!("{} {message}", p.red("error:")),
            EventKind::Finish { status, summary, verified, evidence } => {
                let head = match status {
                    FinishStatus::Done if *verified => p.green("✔ done"),
                    FinishStatus::Done => p.yellow("✔ done, unverified"),
                    FinishStatus::Yielded => p.cyan("◂ handed back"),
                    FinishStatus::Stopped => p.yellow("■ stopped"),
                    FinishStatus::Budget => p.yellow("■ out of budget"),
                    FinishStatus::Error => p.red("✗ failed"),
                };
                eprintln!("{head} {}", p.dim(&format!("({evidence})")));
                if !summary.trim().is_empty() {
                    eprintln!("{}", indent(summary.trim(), "  ", 40));
                }
            }
            _ => {}
        }
    }
}

struct RunArgs {
    req: StartRequest,
    verbose: bool,
    json: bool,
    events: bool,
}

/// Options of `run`. `prompt` takes the same ones and no task.
fn parse_run(args: &[String], needs_task: bool) -> Result<RunArgs, String> {
    let mut req = StartRequest { cwd: ".".into(), ..Default::default() };
    let (mut verbose, mut json, mut events) = (false, false, false);
    let mut task: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "-C" | "--cwd" => req.cwd = value(a)?,
            "-p" | "--provider" => req.provider = Some(value(a)?),
            "-m" | "--model" => req.model = Some(value(a)?),
            "-e" | "--effort" => req.effort = Some(value(a)?),
            "--classic" => req.mode = Some("classic".into()),
            "--without" => req.without.push(value(a)?),
            "--skill" => req.skills.extend(value(a)?.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty())),
            "--system-file" => {
                let path = value(a)?;
                req.system_prompt = Some(std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?);
            }
            "--no-project-instructions" => req.project_instructions = Some(false),
            "--max-turns" => req.max_turns = Some(value(a)?.parse().map_err(|_| "--max-turns needs a number")?),
            "--time-budget" => req.time_budget_secs = Some(value(a)?.parse().map_err(|_| "--time-budget needs a number of seconds")?),
            "-v" | "--verbose" => verbose = true,
            "--json" => json = true,
            "--events" => events = true,
            "--" => task.extend(it.by_ref().cloned()),
            flag if flag.starts_with('-') && flag.len() > 1 => return Err(format!("unknown option {flag}")),
            word => task.push(word.to_string()),
        }
    }
    req.task = task.join(" ");
    if !needs_task {
        return Ok(RunArgs { req, verbose, json, events });
    }
    if req.task.trim() == "-" || req.task.trim().is_empty() {
        let mut buf = String::new();
        if !std::io::stdin().is_terminal() {
            std::io::stdin().read_to_string(&mut buf).map_err(|e| e.to_string())?;
        }
        req.task = buf;
    }
    if req.task.trim().is_empty() {
        return Err("no task given (pass it as arguments or on stdin)".into());
    }
    Ok(RunArgs { req, verbose, json, events })
}

struct Follower {
    printer: Printer,
    events: bool,
    stats: Stats,
    result: Option<(FinishStatus, String, bool, String)>,
}

impl Follower {
    fn handle(&mut self, ev: &Event) {
        if self.events && !ev.kind.transient() {
            println!("{}", serde_json::to_string(ev).unwrap_or_default());
        }
        self.printer.print(ev);
        match &ev.kind {
            EventKind::Stats { stats } => self.stats = stats.clone(),
            EventKind::Finish { status, summary, verified, evidence } => self.result = Some((*status, summary.clone(), *verified, evidence.clone())),
            _ => {}
        }
    }
}

/// Follows a session to the end of its current run. Returns the process exit code.
async fn follow(h: &Harness, id: &str, simulated: bool, verbose: bool, json: bool, events: bool) -> ExitCode {
    let (backlog, mut rx) = match h.subscribe(id) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let printer = Printer { paint: Paint { on: std::io::stderr().is_terminal() }, verbose, pending: None };
    let mut f = Follower { printer, events, stats: Stats::default(), result: None };
    for ev in &backlog {
        f.handle(ev);
    }
    let mut interrupted = false;
    while f.result.is_none() {
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(ev) => f.handle(&ev),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            _ = tokio::signal::ctrl_c() => {
                if interrupted {
                    return ExitCode::from(130);
                }
                interrupted = true;
                eprintln!("stopping… (Ctrl-C again to quit immediately)");
                let _ = h.stop(id);
            }
        }
    }
    eprintln!("{}", f.printer.stats_block(&f.stats, simulated));
    let Some((status, summary, verified, evidence)) = f.result else { return ExitCode::from(2) };
    if json {
        let out = serde_json::json!({"id": id, "status": status, "verified": verified, "evidence": evidence, "summary": summary, "stats": f.stats, "simulated": simulated});
        println!("{out}");
    }
    let _ = std::io::stdout().flush();
    match status {
        FinishStatus::Done | FinishStatus::Yielded => ExitCode::SUCCESS,
        _ => ExitCode::from(2),
    }
}

fn replay(h: &Harness, id: &str, verbose: bool) -> ExitCode {
    match h.events(id) {
        Ok(events) => {
            let mut printer = Printer { paint: Paint { on: std::io::stderr().is_terminal() }, verbose, pending: None };
            let mut stats = Stats::default();
            let mut simulated = false;
            for ev in &events {
                printer.print(ev);
                match &ev.kind {
                    EventKind::Stats { stats: s } => stats = s.clone(),
                    EventKind::SessionStart { simulated: s, .. } => simulated = *s,
                    _ => {}
                }
            }
            eprintln!("{}", printer.stats_block(&stats, simulated));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let h = Harness::new();
    let fail = |msg: String| {
        eprintln!("error: {msg}");
        ExitCode::from(2)
    };
    match args.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        Some("-V" | "--version" | "version") => {
            println!("presage {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("skills") => match (args.get(1).map(String::as_str), args.get(2)) {
            (None, _) => {
                for s in h.skills(Some(".")) {
                    println!("{:<14} {:<8} {}", s.name, s.source.as_str(), s.description);
                }
                ExitCode::SUCCESS
            }
            (Some("show"), Some(name)) => match h.skill_file(name, Some(".")) {
                Ok(text) => {
                    println!("{}", text.trim_end());
                    ExitCode::SUCCESS
                }
                Err(e) => fail(e),
            },
            _ => fail("usage: presage skills [show <name>]".into()),
        },
        Some("prompt") => match parse_run(&args[1..], false).and_then(|run| h.system_prompt(&run.req)) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(e) => fail(e),
        },
        Some("run") => match parse_run(&args[1..], true) {
            Err(e) => fail(e),
            Ok(run) => match h.start(run.req) {
                Err(e) => fail(e),
                Ok(meta) => follow(&h, &meta.id, meta.simulated, run.verbose, run.json, run.events).await,
            },
        },
        Some("demo") => match h.demo() {
            Err(e) => fail(e),
            Ok(meta) => follow(&h, &meta.id, true, args.iter().any(|a| a == "-v" || a == "--verbose"), args.iter().any(|a| a == "--json"), false).await,
        },
        Some("bench") => bench::command(&h, &args[1..]).await,
        Some("sessions") => {
            for m in h.list() {
                let state = match (m.status, m.outcome, m.verified) {
                    (SessionStatus::Running, _, _) => "running",
                    (_, Some(FinishStatus::Done), Some(true)) => "done ✓",
                    (_, Some(FinishStatus::Done), _) => "done ?",
                    (_, Some(FinishStatus::Yielded), _) => "handed back",
                    (_, Some(FinishStatus::Stopped), _) => "stopped",
                    (_, Some(FinishStatus::Budget), _) => "out of budget",
                    (_, Some(FinishStatus::Error), _) => "failed",
                    _ => "–",
                };
                println!("{}  {:<13} {:>3} turns  {:<26} {}", m.id, state, m.stats.turns, m.model, m.title);
            }
            ExitCode::SUCCESS
        }
        Some("show") => match args.get(1) {
            Some(id) => replay(&h, id, args.iter().any(|a| a == "-v" || a == "--verbose")),
            None => fail("usage: presage show <session-id>".into()),
        },
        Some("providers") => {
            let state = h.state();
            for p in &state.providers {
                let key = if p.has_key { "key set" } else { "no key" };
                println!("{:<11} {:<8} {:<38} {}  [{}]", p.id, key, p.base_url, p.models.join(", "), p.key_env);
            }
            if let (Some(p), Some(m)) = (&state.provider, &state.model) {
                println!("default: {m} via {p} · effort {}", state.effort);
            }
            ExitCode::SUCCESS
        }
        Some("key") => match args.get(1) {
            None => fail("usage: presage key <provider>   (the key is read from stdin)".into()),
            Some(provider) => {
                if std::io::stdin().is_terminal() {
                    eprint!("API key for {provider} (input is not hidden): ");
                }
                let mut key = String::new();
                if std::io::stdin().read_line(&mut key).is_err() {
                    return fail("could not read the key from stdin".into());
                }
                match h.set_key(provider, &key) {
                    Ok(()) => {
                        eprintln!("{}", if key.trim().is_empty() { "key removed" } else { "key stored in ~/.presage/config.json (owner-readable only)" });
                        ExitCode::SUCCESS
                    }
                    Err(e) => fail(e),
                }
            }
        },
        Some(other) => fail(format!("unknown command {other:?}; try `presage --help`")),
    }
}
