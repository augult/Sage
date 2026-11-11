//! `presage bench`: the built-in tasks run under several harness variants, every run verified
//! independently of what the agent claimed, summarised as medians.

mod external;
mod proxy;
mod tasks;

use crate::{k, pct};
use presage_core::config::Config;
use presage_core::economy::{peak_multiplier, prices_for};
use presage_core::events::{Event, EventKind, FinishStatus, Stats};
use presage_core::harness::{Harness, StartRequest};
use presage_core::prompt::prefix_tokens;
use presage_core::provider::mock::{MockProvider, MockTurn};
use presage_core::state::Features;
use presage_core::util::{civil_date, fmt_duration, now_ms, one_line, rand_hex};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tasks::{TASKS, Task};
use tokio::sync::Semaphore;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinSet;

const HELP: &str = "\
presage bench — run the built-in tasks under several harness variants and compare them

USAGE
  presage bench [options]
  presage bench --list          show the tasks
  presage bench --check         check the tasks themselves: each must fail as shipped and pass with
                                its reference solution (no model involved)

OPTIONS
  -p, --provider <id>      provider (default: the configured one)
  -m, --model <name>       model; repeat or separate with commas to compare several
      --variants <list>    presage, classic, no-<mechanism>, e.g. no-auto-watch (default: presage,classic);
                           add +skill (each task with its skill), +sys:<file> (system prompt template),
                           +effort:<level> or +no-<mechanism>, e.g. presage+skill+effort:max
                           ext:<name> for another harness, e.g. ext:mini, ext:opencode, ext:aider
      --external-dir <dir> where ext:<name> finds <name>/run.sh (default: scripts/external)
      --tasks <list>       task ids (default: all)
  -n, --repeats <n>        runs per task and variant (default 3)
  -e, --effort <level>     as for `presage run`
      --max-turns <n>      per run (default 30)
      --time-budget <s>    per run (default 600)
  -j, --jobs <n>           runs in parallel (default 4)
      --max-cost <usd>     start no further runs once this much is spent at list price
      --retries <n>        extra attempts for a run the provider failed (default 1)
      --out <file>         also write the full results as JSON to this file
      --keep               keep the workspaces of the runs
      --scripted           replace the model by a stand-in that applies each reference solution:
                           checks the benchmark offline, measures nothing

Every run starts from a fresh copy of the task. When the agent is done, the files it was told to
leave alone are put back, held-out checks are added and the task's verification is run. Only that
counts as solved, whatever the agent claimed.

An ext:<name> run reaches the model through a local proxy that counts its requests and tokens and
refuses requests beyond --max-turns, and is killed at --time-budget. Its cost is at Presage's
prices. Install the harnesses with scripts/external/install.sh.";

const VERIFY_TIMEOUT: Duration = Duration::from_secs(120);
/// Name of the summary row that spans every task.
pub const ALL: &str = "ALL";

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Variant {
    pub name: String,
    pub classic: bool,
    /// Mechanisms switched off, by their `Features` name.
    pub without: Vec<String>,
    /// Run each task with the skill it names (`+skill`).
    pub skill: bool,
    /// A system prompt template that replaces the built-in one (`+sys:<file>`).
    #[serde(skip)]
    pub system: Option<String>,
    /// Reasoning effort for this variant only (`+effort:<level>`).
    pub effort: Option<String>,
    /// Another harness, run through its adapter `<name>/run.sh`, instead of Presage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external: Option<String>,
}

impl Variant {
    fn own(name: &str, classic: bool, without: Vec<String>) -> Variant {
        Variant { name: name.to_string(), classic, without, ..Variant::default() }
    }

    pub fn parse(s: &str) -> Result<Variant, String> {
        let name = s.trim().to_string();
        if let Some(harness) = name.to_ascii_lowercase().strip_prefix("ext:") {
            if harness.is_empty() || !harness.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return Err(format!("{s:?} does not name an external harness: use ext:<name>, with letters, digits, - and _"));
            }
            return Ok(Variant { external: Some(harness.to_string()), ..Variant::own(&name.to_ascii_lowercase(), false, vec![]) });
        }
        let unknown = || format!("unknown variant {s:?}: use presage, classic, or no-<mechanism> with one of {}; modifiers +skill, +sys:<file>, +effort:<level>, +no-<mechanism>", Features::NAMES.join(", "));
        let mechanism = |m: &str| -> Result<String, String> {
            let mechanism = m.to_ascii_lowercase().strip_prefix("no-").ok_or_else(unknown)?.replace('-', "_");
            Features::presage().without(&mechanism).ok_or_else(unknown)?;
            Ok(mechanism)
        };
        let mut parts = name.split('+');
        let base = parts.next().unwrap_or_default().to_ascii_lowercase();
        let mut v = match base.as_str() {
            "presage" => Variant::default(),
            "classic" => Variant { classic: true, ..Variant::default() },
            other => Variant { without: vec![mechanism(other)?], ..Variant::default() },
        };
        for m in parts {
            if m == "skill" {
                v.skill = true;
            } else if let Some(path) = m.strip_prefix("sys:") {
                v.system = Some(std::fs::read_to_string(path).map_err(|e| format!("variant {s:?}: cannot read {path}: {e}"))?);
            } else if let Some(level) = m.strip_prefix("effort:") {
                v.effort = Some(level.to_string());
            } else {
                v.without.push(mechanism(m)?);
            }
        }
        v.name = if v.system.is_some() { name } else { name.to_ascii_lowercase() };
        Ok(v)
    }

    fn features(&self) -> Features {
        let base = if self.classic { Features::classic() } else { Features::presage() };
        self.without.iter().fold(base, |f, name| f.without(name).unwrap_or(f))
    }
}

pub struct Options {
    pub provider: Option<String>,
    pub models: Vec<String>,
    pub variants: Vec<Variant>,
    pub tasks: Vec<&'static Task>,
    pub repeats: u32,
    pub effort: Option<String>,
    pub max_turns: u32,
    pub time_budget: u64,
    pub jobs: usize,
    pub max_cost: Option<f64>,
    pub retries: u32,
    pub out: Option<PathBuf>,
    pub keep: bool,
    pub scripted: bool,
    /// Where the adapters of external harnesses are; `None` for the ones shipped with the source.
    pub external_dir: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            provider: None,
            models: Vec::new(),
            variants: vec![Variant::own("presage", false, vec![]), Variant::own("classic", true, vec![])],
            tasks: TASKS.iter().collect(),
            repeats: 3,
            effort: None,
            max_turns: 30,
            time_budget: 600,
            jobs: 4,
            max_cost: None,
            retries: 1,
            out: None,
            keep: false,
            scripted: false,
            external_dir: None,
        }
    }
}

enum Command {
    Help,
    List,
    Check(Vec<&'static Task>),
    Run(Options),
}

fn list_of(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|v| !v.is_empty()).map(String::from).collect()
}

fn number<T: std::str::FromStr>(flag: &str, value: String) -> Result<T, String> {
    value.trim().parse().map_err(|_| format!("{flag} needs a number, not {value:?}"))
}

fn parse(args: &[String]) -> Result<Command, String> {
    let mut opt = Options::default();
    let (mut list, mut check) = (false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "-h" | "--help" | "help" => return Ok(Command::Help),
            "--list" => list = true,
            "--check" => check = true,
            "-p" | "--provider" => opt.provider = Some(value(a)?),
            "-m" | "--model" | "--models" => opt.models.extend(list_of(&value(a)?)),
            "--variants" | "--variant" => opt.variants = list_of(&value(a)?).iter().map(|v| Variant::parse(v)).collect::<Result<_, _>>()?,
            "--tasks" | "--task" => {
                // A name is a task id, a set (`easy`, `hard`, `skill`) or `all`.
                let mut picked: Vec<&'static Task> = Vec::new();
                for id in list_of(&value(a)?) {
                    if id == "all" {
                        picked.extend(tasks::all());
                    } else if let Some((_, set)) = tasks::SETS.iter().find(|(name, _)| *name == id) {
                        picked.extend(set.iter());
                    } else {
                        picked.push(tasks::find(&id).ok_or_else(|| format!("unknown task {id:?}; `presage bench --list` shows them"))?);
                    }
                }
                opt.tasks = picked;
            }
            "-n" | "--repeats" => opt.repeats = number(a, value(a)?)?,
            "-e" | "--effort" => opt.effort = Some(value(a)?),
            "--max-turns" => opt.max_turns = number(a, value(a)?)?,
            "--time-budget" => opt.time_budget = number(a, value(a)?)?,
            "-j" | "--jobs" => opt.jobs = number(a, value(a)?)?,
            "--max-cost" => opt.max_cost = Some(number(a, value(a)?)?),
            "--retries" => opt.retries = number(a, value(a)?)?,
            "--out" => opt.out = Some(PathBuf::from(value(a)?)),
            "--keep" => opt.keep = true,
            "--scripted" => opt.scripted = true,
            "--external-dir" => opt.external_dir = Some(PathBuf::from(value(a)?)),
            other => return Err(format!("unknown option {other}; try `presage bench --help`")),
        }
    }
    if opt.repeats == 0 || opt.jobs == 0 || opt.max_turns == 0 || opt.tasks.is_empty() || opt.variants.is_empty() {
        return Err("--repeats, --jobs and --max-turns must be at least 1, and at least one task and one variant are needed".into());
    }
    Ok(if list {
        Command::List
    } else if check {
        Command::Check(opt.tasks)
    } else {
        Command::Run(opt)
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct Run {
    #[serde(skip)]
    seq: usize,
    pub model: String,
    pub task: String,
    pub variant: String,
    pub repeat: u32,
    /// Its log is `<dir>/home/sessions/<session>/events.jsonl`; for an external harness,
    /// `<dir>/external/<session>/output.log`.
    pub session: String,
    /// How the agent's run ended: done, yielded, stopped, budget or error.
    pub outcome: String,
    /// The harness accepted `finish` as verified by its own gate.
    pub harness_verified: bool,
    /// The task's verification passed afterwards. Nothing else counts as solved.
    pub solved: bool,
    /// Why the run itself failed, when the model (not the provider) was the cause.
    pub failure: Option<String>,
    /// Last lines of the verification output when it failed.
    pub verify_tail: String,
    /// Set when the provider failed and the run has no result. Such runs are left out of the medians.
    pub error: Option<String>,
    /// What the run would have cost at list price had the provider served nothing from its cache.
    /// Takes the luck of the cache out of a comparison.
    pub cost_uncached: f64,
    /// For an external harness, turns, tokens, cost and times as its proxy measured them; the
    /// rest is Presage's own and stays empty.
    pub stats: Stats,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external: Option<external::Detail>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Medians {
    pub turns: f64,
    pub steps: f64,
    pub prompt_tokens: f64,
    pub cached_tokens: f64,
    pub output_tokens: f64,
    pub reasoning_tokens: f64,
    pub cost: f64,
    pub cost_uncached: f64,
    pub elapsed_ms: f64,
}

/// One row of the summary: a task (or `ALL`) under one variant on one model.
#[derive(Clone, Debug, Serialize)]
pub struct Cell {
    pub model: String,
    pub task: String,
    pub variant: String,
    /// Runs that produced a result.
    pub runs: usize,
    /// Runs lost to provider failures.
    pub errors: usize,
    pub solved: usize,
    /// Runs the agent declared done that then failed verification.
    pub false_done: usize,
    pub total_cost: f64,
    pub median: Medians,
}

#[derive(Clone, Debug, Serialize)]
pub struct Prefix {
    pub variant: String,
    pub tokens: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub id: String,
    pub started_ms: u64,
    pub provider: String,
    pub effort: String,
    pub repeats: u32,
    /// True when a scripted stand-in played the model: every number is then fabricated.
    pub simulated: bool,
    pub dir: String,
    pub models: Vec<String>,
    pub tasks: Vec<String>,
    pub variants: Vec<String>,
    /// Estimated size of the system prompt plus tool schemas, per variant.
    pub prefix_tokens: Vec<Prefix>,
    /// Median measured size of the first prompt of each external harness: its system prompt, its
    /// tool schemas and the task.
    pub first_prompt_tokens: Vec<Prefix>,
    pub skipped_tasks: Vec<String>,
    /// Runs never started because the cost limit was reached.
    pub not_started: usize,
    pub summary: Vec<Cell>,
    pub runs: Vec<Run>,
}

struct Job {
    seq: usize,
    model: String,
    task: &'static Task,
    variant: Variant,
    repeat: u32,
}

struct Shared {
    harness: Harness,
    dir: PathBuf,
    provider: Option<String>,
    effort: Option<String>,
    max_turns: u32,
    time_budget: u64,
    max_cost: Option<f64>,
    retries: u32,
    keep: bool,
    scripted: bool,
    progress: bool,
    total: usize,
    slots: Semaphore,
    spent: Mutex<f64>,
    done: Mutex<usize>,
    /// The adapters' directory and the proxy they reach the model through, when a variant needs them.
    external: Option<(PathBuf, proxy::Proxy)>,
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

fn bench_id() -> String {
    let secs = now_ms() / 1000;
    let (y, m, d) = civil_date(secs);
    let t = secs % 86_400;
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}-{}", t / 3600, (t % 3600) / 60, t % 60, rand_hex(2))
}

fn write_file(root: &Path, rel: &str, content: &str) -> std::io::Result<()> {
    let path = root.join(rel);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, content)
}

fn materialize(root: &Path, files: &[(&str, &str)]) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    files.iter().try_for_each(|(path, content)| write_file(root, path, content))
}

/// Judges a workspace: puts back the files the agent was told to leave alone, adds the held-out
/// checks and runs the task's verification. Returns whether it passed and, if not, its last lines.
async fn verify(task: &Task, ws: &Path) -> (bool, String) {
    let restored = task.restore.iter().filter_map(|path| task.files.iter().find(|(p, _)| p == path));
    for (path, content) in restored.chain(task.hidden.iter()) {
        if let Err(e) = write_file(ws, path, content) {
            return (false, format!("could not write {path}: {e}"));
        }
    }
    let child = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(task.verify)
        .current_dir(ws)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let child = match child {
        Ok(c) => c,
        Err(e) => return (false, format!("could not start bash: {e}")),
    };
    match tokio::time::timeout(VERIFY_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(out)) if out.status.success() => (true, String::new()),
        Ok(Ok(out)) => {
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let lines: Vec<&str> = text.lines().collect();
            (false, lines[lines.len().saturating_sub(12)..].join("\n"))
        }
        Ok(Err(e)) => (false, e.to_string()),
        Err(_) => (false, format!("verification did not finish within {}s", VERIFY_TIMEOUT.as_secs())),
    }
}

/// Proves the tasks themselves: each must fail as shipped and pass with its reference solution.
pub async fn check(tasks: &[&'static Task]) -> Vec<(&'static str, Result<(), String>)> {
    let base = std::env::temp_dir().join(format!("presage-bench-check-{}", rand_hex(4)));
    let mut verdicts = Vec::new();
    for task in tasks {
        let verdict = async {
            let shipped = base.join(task.id).join("shipped");
            materialize(&shipped, task.files).map_err(|e| e.to_string())?;
            if verify(task, &shipped).await.0 {
                return Err("passes its verification as shipped, so there is nothing to solve".to_string());
            }
            let solved = base.join(task.id).join("solved");
            materialize(&solved, task.files).and_then(|_| materialize(&solved, task.solution)).map_err(|e| e.to_string())?;
            match verify(task, &solved).await {
                (true, _) => Ok(()),
                (false, tail) => Err(format!("its reference solution fails verification:\n{tail}")),
            }
        }
        .await;
        verdicts.push((task.id, verdict));
    }
    let _ = std::fs::remove_dir_all(&base);
    verdicts
}

/// A stand-in model that writes the reference solution, runs the task's check and finishes.
fn stand_in(task: &Task) -> MockProvider {
    let mut calls: Vec<(&str, Value)> = task.solution.iter().map(|(path, content)| ("write", json!({"path": path, "content": content, "overwrite": true}))).collect();
    calls.push(("shell", json!({"cmd": task.check, "expect_exit": "0"})));
    calls.push(("finish", json!({"summary": "Applied the reference solution."})));
    // The second reply answers the review pass, where the variant has one.
    MockProvider::scripted(vec![MockTurn::calls("Applying the reference solution.", calls), MockTurn::calls("", vec![("finish", json!({"summary": "Applied the reference solution."}))])])
}

struct End {
    status: FinishStatus,
    verified: bool,
    stats: Stats,
    error: Option<String>,
}

fn is_finish(ev: &Event) -> bool {
    matches!(ev.kind, EventKind::Finish { .. })
}

/// Waits for the session's run to end, then reads the result off its stored events.
async fn wait_for_end(h: &Harness, id: &str) -> Result<End, String> {
    let (backlog, mut rx) = h.subscribe(id)?;
    let mut finished = backlog.iter().any(is_finish);
    while !finished {
        finished = match rx.recv().await {
            Ok(ev) => is_finish(&ev),
            // Events were dropped from the live feed; the stored log has them all.
            Err(RecvError::Lagged(_)) => h.events(id)?.iter().any(is_finish),
            Err(RecvError::Closed) => return Err("the session ended without a result".into()),
        };
    }
    let (mut stats, mut error, mut end) = (Stats::default(), None, None);
    for ev in h.events(id)? {
        match ev.kind {
            EventKind::Stats { stats: s } => stats = s,
            EventKind::Error { message } => error = Some(message),
            EventKind::Finish { status, verified, .. } => end = Some((status, verified)),
            _ => {}
        }
    }
    let (status, verified) = end.ok_or("the session ended without a result")?;
    Ok(End { status, verified, stats, error })
}

/// Whether a failed run failed because the provider did rather than the model. Follows the wording
/// of `ProviderError`.
fn provider_failed(message: &str) -> bool {
    ["provider returned HTTP", "network error", "unexpected provider response"].iter().any(|p| message.starts_with(p))
}

fn outcome_name(status: FinishStatus) -> &'static str {
    match status {
        FinishStatus::Done => "done",
        FinishStatus::Yielded => "yielded",
        FinishStatus::Stopped => "stopped",
        FinishStatus::Budget => "budget",
        FinishStatus::Error => "error",
    }
}

async fn attempt(shared: &Shared, job: &Job, retry: u32) -> Run {
    let mut run = Run {
        seq: job.seq,
        model: job.model.clone(),
        task: job.task.id.to_string(),
        variant: job.variant.name.clone(),
        repeat: job.repeat,
        session: String::new(),
        outcome: "none".into(),
        harness_verified: false,
        solved: false,
        failure: None,
        verify_tail: String::new(),
        error: None,
        cost_uncached: 0.0,
        stats: Stats::default(),
        external: None,
    };
    let label = format!("{}-{}-{}-{}", job.seq, job.task.id, job.variant.name.replace(':', "-"), retry);
    let ws = shared.dir.join("work").join(&label);
    if let Err(e) = materialize(&ws, job.task.files) {
        run.error = Some(format!("could not create the workspace: {e}"));
        return run;
    }
    if let (Some(name), Some((dir, proxy))) = (&job.variant.external, &shared.external) {
        let script = external::script(dir, name);
        let home = shared.dir.join("external").join(&label);
        let ext = external::Job { id: &label, script: &script, workspace: &ws, home: &home, prompt: job.task.prompt, model: &job.model, max_turns: shared.max_turns, time_budget: shared.time_budget, keep: shared.keep };
        let ran = external::run(proxy, &ext).await;
        let u = &ran.detail.usage;
        run.session = label;
        run.outcome = ran.outcome.to_string();
        (run.failure, run.error) = (ran.failure, ran.error);
        run.cost_uncached = u.cost_uncached;
        run.stats = Stats {
            turns: u.turns,
            prompt_tokens: u.prompt_tokens,
            cached_tokens: u.cached_tokens,
            output_tokens: u.output_tokens,
            reasoning_tokens: u.reasoning_tokens,
            context_tokens: u.context_tokens,
            cost: u.cost,
            priced: u.priced,
            elapsed_ms: ran.elapsed_ms,
            model_ms: u.model_ms,
            ..Stats::default()
        };
        run.external = Some(ran.detail);
        return judged(shared, job, &ws, run).await;
    }
    let req = StartRequest {
        cwd: ws.to_string_lossy().into_owned(),
        task: job.task.prompt.to_string(),
        provider: shared.provider.clone(),
        model: (!shared.scripted).then(|| job.model.clone()),
        effort: job.variant.effort.clone().or_else(|| shared.effort.clone()),
        mode: Some(if job.variant.classic { "classic" } else { "presage" }.to_string()),
        autonomy: None,
        max_turns: Some(shared.max_turns),
        time_budget_secs: Some(shared.time_budget),
        interactive: false,
        without: job.variant.without.clone(),
        system_prompt: job.variant.system.clone(),
        skills: if job.variant.skill && !job.task.skill.is_empty() { vec![job.task.skill.to_string()] } else { Vec::new() },
        ..Default::default()
    };
    let started = if shared.scripted { shared.harness.start_scripted(req, stand_in(job.task)) } else { shared.harness.start(req) };
    match started {
        Err(e) => run.error = Some(e),
        Ok(meta) => {
            run.session = meta.id.clone();
            match wait_for_end(&shared.harness, &meta.id).await {
                Err(e) => run.error = Some(e),
                Ok(end) => {
                    run.outcome = outcome_name(end.status).to_string();
                    run.harness_verified = end.status == FinishStatus::Done && end.verified;
                    run.stats = end.stats;
                    let refund = prices_for(&job.model).map_or(0.0, |p| run.stats.cached_tokens as f64 * (p.miss - p.hit) / 1e6);
                    run.cost_uncached = run.stats.cost + refund * peak_multiplier(&job.model, now_ms() / 1000);
                    if end.status == FinishStatus::Error {
                        let message = end.error.unwrap_or_else(|| "the run failed".into());
                        // A provider that failed gave no result; a model that failed gave a bad one.
                        if provider_failed(&message) { run.error = Some(message) } else { run.failure = Some(message) }
                    }
                }
            }
            let _ = shared.harness.close(&meta.id);
        }
    }
    judged(shared, job, &ws, run).await
}

/// The same verdict for every harness: the run's workspace is verified unless the provider failed.
async fn judged(shared: &Shared, job: &Job, ws: &Path, mut run: Run) -> Run {
    if run.error.is_none() {
        (run.solved, run.verify_tail) = verify(job.task, ws).await;
    }
    if !shared.keep {
        let _ = std::fs::remove_dir_all(ws);
    }
    run
}

/// Runs one cell of the matrix, retrying when the provider failed. `None` when the cost limit had
/// already been reached.
async fn run_job(shared: Arc<Shared>, job: Job) -> Option<Run> {
    let _slot = shared.slots.acquire().await.ok()?;
    if shared.max_cost.is_some_and(|limit| *shared.spent.lock().unwrap() >= limit) {
        return None;
    }
    let mut retry = 0;
    let run = loop {
        let run = attempt(&shared, &job, retry).await;
        *shared.spent.lock().unwrap() += run.stats.cost;
        if run.error.is_none() || retry >= shared.retries {
            break run;
        }
        retry += 1;
    };
    if shared.progress {
        let n = {
            let mut done = shared.done.lock().unwrap();
            *done += 1;
            *done
        };
        let verdict = match (&run.error, run.solved) {
            (Some(e), _) => format!("no result ({})", one_line(e, 90)),
            (None, true) => "solved".to_string(),
            (None, false) => format!("NOT solved ({}{})", run.outcome, run.failure.as_ref().map(|f| format!(": {}", one_line(f, 90))).unwrap_or_default()),
        };
        eprintln!("[{n}/{}] {} · {} · {} #{}: {verdict} · {} turns · ${:.4} · {}", shared.total, job.model, job.task.id, job.variant.name, job.repeat, run.stats.turns, run.stats.cost, fmt_duration(run.stats.elapsed_ms));
    }
    Some(run)
}

fn median(mut xs: Vec<f64>) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.sort_by(|a, b| a.total_cmp(b));
    let mid = xs.len() / 2;
    if xs.len() % 2 == 1 { xs[mid] } else { (xs[mid - 1] + xs[mid]) / 2.0 }
}

fn cell(model: &str, task: &str, variant: &str, runs: &[&Run]) -> Cell {
    let done: Vec<&Run> = runs.iter().copied().filter(|r| r.error.is_none()).collect();
    let med = |value: fn(&Stats) -> f64| median(done.iter().map(|r| value(&r.stats)).collect());
    Cell {
        model: model.to_string(),
        task: task.to_string(),
        variant: variant.to_string(),
        runs: done.len(),
        errors: runs.len() - done.len(),
        solved: done.iter().filter(|r| r.solved).count(),
        false_done: done.iter().filter(|r| r.outcome == "done" && !r.solved).count(),
        total_cost: done.iter().map(|r| r.stats.cost).sum(),
        median: Medians {
            turns: med(|s| s.turns as f64),
            steps: med(|s| s.steps as f64),
            prompt_tokens: med(|s| s.prompt_tokens as f64),
            cached_tokens: med(|s| s.cached_tokens as f64),
            output_tokens: med(|s| s.output_tokens as f64),
            reasoning_tokens: med(|s| s.reasoning_tokens as f64),
            cost: med(|s| s.cost),
            cost_uncached: median(done.iter().map(|r| r.cost_uncached).collect()),
            elapsed_ms: med(|s| s.elapsed_ms as f64),
        },
    }
}

fn summarise(runs: &[Run], models: &[String], tasks: &[String], variants: &[String]) -> Vec<Cell> {
    let mut cells = Vec::new();
    for model in models {
        for task in tasks.iter().map(String::as_str).chain([ALL]) {
            for variant in variants {
                let of: Vec<&Run> = runs.iter().filter(|r| &r.model == model && &r.variant == variant && (task == ALL || r.task == task)).collect();
                cells.push(cell(model, task, variant, &of));
            }
        }
    }
    cells
}

/// Runs the whole matrix and returns the report, which is also written to `<dir>/results.json`.
pub async fn execute(h: &Harness, opt: &Options, progress: bool) -> Result<Report, String> {
    if cfg!(not(unix)) {
        return Err("the benchmark tasks need a unix shell".into());
    }
    let state = h.state();
    let (provider, models) = if opt.scripted {
        (None, vec!["scripted stand-in".to_string()])
    } else {
        let id = opt.provider.clone().or_else(|| state.provider.clone()).ok_or("no provider has an API key yet: add one (see `presage providers`), or pass --scripted")?;
        let info = state.providers.iter().find(|p| p.id == id).ok_or_else(|| format!("unknown provider {id:?}"))?;
        if !info.has_key {
            return Err(format!("no API key for {}: set {} or run `presage key {}`", info.name, info.key_env, info.id));
        }
        let default = if opt.provider.is_none() { state.model.clone() } else { info.models.first().cloned() };
        let models: Vec<String> = if opt.models.is_empty() { default.into_iter().collect() } else { opt.models.clone() };
        if models.is_empty() {
            return Err(format!("no model given and {} has no default one", info.name));
        }
        (Some(id), models)
    };
    let (tasks, skipped): (Vec<&'static Task>, Vec<&'static Task>) = opt.tasks.iter().copied().partition(|t| t.needs.iter().all(|p| on_path(p)));
    if tasks.is_empty() {
        return Err("none of the chosen tasks can run here: a program they need is not installed".into());
    }

    let external = if let Some(first) = opt.variants.iter().find(|v| v.external.is_some()) {
        let provider = provider.as_deref().ok_or_else(|| format!("{} needs a real model: drop --scripted", first.name))?;
        let adapters = opt.external_dir.clone().unwrap_or_else(external::default_dir);
        let adapters = adapters.canonicalize().map_err(|e| format!("no adapters at {}: {e}", adapters.display()))?;
        for name in opt.variants.iter().filter_map(|v| v.external.as_deref()) {
            let script = external::script(&adapters, name);
            if !script.is_file() {
                return Err(format!("no adapter for ext:{name}: {} does not exist", script.display()));
            }
        }
        let endpoint = Config::load(&Path::new(&state.data_dir).join("config.json")).resolve(Some(provider), None)?;
        Some((adapters, proxy::Proxy::start(&endpoint.base_url, &endpoint.key).await?))
    } else {
        None
    };

    let id = bench_id();
    let dir = PathBuf::from(&state.data_dir).join("bench").join(&id);
    std::fs::create_dir_all(dir.join("work")).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let shared = Arc::new(Shared {
        harness: h.with_data_dir(dir.join("home")),
        dir: dir.clone(),
        provider: provider.clone(),
        effort: opt.effort.clone(),
        max_turns: opt.max_turns,
        time_budget: opt.time_budget,
        max_cost: opt.max_cost,
        retries: opt.retries,
        keep: opt.keep,
        scripted: opt.scripted,
        progress,
        total: models.len() * opt.repeats as usize * tasks.len() * opt.variants.len(),
        slots: Semaphore::new(opt.jobs),
        spent: Mutex::new(0.0),
        done: Mutex::new(0),
        external,
    });

    // Variants alternate within each repeat, so that drifting provider load hits them alike.
    let started_ms = now_ms();
    let mut set = JoinSet::new();
    let mut seq = 0;
    for model in &models {
        for repeat in 1..=opt.repeats {
            for &task in &tasks {
                for variant in &opt.variants {
                    set.spawn(run_job(shared.clone(), Job { seq, model: model.clone(), task, variant: variant.clone(), repeat }));
                    seq += 1;
                }
            }
        }
    }
    let (mut runs, mut not_started) = (Vec::new(), 0);
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(Some(run)) => runs.push(run),
            Ok(None) => not_started += 1,
            Err(e) => return Err(format!("a benchmark run crashed: {e}")),
        }
    }
    runs.sort_by_key(|r| r.seq);

    let task_ids: Vec<String> = tasks.iter().map(|t| t.id.to_string()).collect();
    let variant_names: Vec<String> = opt.variants.iter().map(|v| v.name.clone()).collect();
    let first_prompt = |variant: &Variant| {
        let sizes = runs.iter().filter(|r| r.variant == variant.name).filter_map(|r| r.external.as_ref()).map(|d| d.usage.first_prompt_tokens as f64).filter(|n| *n > 0.0);
        Prefix { variant: variant.name.clone(), tokens: median(sizes.collect()) as u64 }
    };
    let report = Report {
        id,
        started_ms,
        provider: provider.unwrap_or_else(|| "scripted".into()),
        effort: opt.effort.clone().unwrap_or(state.effort),
        repeats: opt.repeats,
        simulated: opt.scripted,
        dir: dir.to_string_lossy().into_owned(),
        prefix_tokens: opt.variants.iter().filter(|v| v.external.is_none()).map(|v| Prefix { variant: v.name.clone(), tokens: prefix_tokens(&v.features()) }).collect(),
        first_prompt_tokens: opt.variants.iter().filter(|v| v.external.is_some()).map(first_prompt).collect(),
        skipped_tasks: skipped.iter().map(|t| t.id.to_string()).collect(),
        not_started,
        summary: summarise(&runs, &models, &task_ids, &variant_names),
        models,
        tasks: task_ids,
        variants: variant_names,
        runs,
    };
    let body = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("results.json"), body).map_err(|e| format!("could not write the results: {e}"))?;
    Ok(report)
}

fn num(x: f64) -> String {
    if x.fract() == 0.0 { format!("{x:.0}") } else { format!("{x:.1}") }
}

fn delta(new: f64, old: f64) -> String {
    if old <= 0.0 { "n/a".to_string() } else { format!("{:+.0}%", (new / old - 1.0) * 100.0) }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

pub fn render(r: &Report) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("presage bench {} · {} · effort {} · {} per task and variant", r.id, r.provider, r.effort, plural(r.repeats as usize, "run")));
    if r.simulated {
        out.push("SCRIPTED STAND-IN: every run replays the reference solution. Turns, tokens and costs are fabricated; this checks the benchmark and measures nothing.".into());
    }
    let sizes = |of: &[Prefix]| of.iter().filter(|p| p.tokens > 0).map(|p| format!("{} ~{} tokens", p.variant, p.tokens)).collect::<Vec<_>>().join(", ");
    if !r.prefix_tokens.is_empty() {
        out.push(format!("fixed prompt prefix, estimated: {}", sizes(&r.prefix_tokens)));
    }
    if r.first_prompt_tokens.iter().any(|p| p.tokens > 0) {
        out.push(format!("first prompt with the task, measured: {}", sizes(&r.first_prompt_tokens)));
    }
    let details = || r.runs.iter().filter_map(|run| Some((run, run.external.as_ref()?)));
    for variant in &r.variants {
        let unmetered: u32 = details().filter(|(run, _)| &run.variant == variant).map(|(_, d)| d.usage.unmetered).sum();
        if unmetered > 0 {
            out.push(format!("{variant}: {} answered without a usage report; its tokens and cost are understated", plural(unmetered as usize, "request")));
        }
    }
    if !r.skipped_tasks.is_empty() {
        out.push(format!("skipped, a program they need is not installed: {}", r.skipped_tasks.join(", ")));
    }
    if r.not_started > 0 {
        out.push(format!("{} not started: the --max-cost limit was reached", plural(r.not_started, "run")));
    }

    for model in &r.models {
        let cells = |task: &str, variant: &str| r.summary.iter().find(|c| &c.model == model && c.task == task && c.variant == variant).filter(|c| c.runs > 0);
        out.push(String::new());
        out.push(format!("{model}   medians per run; solved = the held-out verification passed after the run; no cache = cost had the provider cached nothing"));
        out.push(format!("{:<10} {:<16} {:>6} {:>5} {:>7} {:>6} {:>7} {:>9} {:>9} {:>7}", "task", "variant", "solved", "turns", "in tok", "cached", "out tok", "cost", "no cache", "time"));
        let mut previous = "";
        for c in r.summary.iter().filter(|c| &c.model == model) {
            let m = &c.median;
            let label = if c.task == previous { "" } else { c.task.as_str() };
            previous = &c.task;
            let mut line = format!(
                "{label:<10} {:<16} {:>6} {:>5} {:>7} {:>6} {:>7} {:>9} {:>9} {:>7}",
                c.variant,
                format!("{}/{}", c.solved, c.runs),
                num(m.turns),
                k(m.prompt_tokens as u64),
                pct(m.cached_tokens as u64, m.prompt_tokens as u64),
                k(m.output_tokens as u64),
                format!("${:.4}", m.cost),
                format!("${:.4}", m.cost_uncached),
                fmt_duration(m.elapsed_ms as u64)
            );
            if c.task == ALL && c.solved > 0 {
                line.push_str(&format!("   total ${:.3}, ${:.4} per solved task", c.total_cost, c.total_cost / c.solved as f64));
            }
            if c.false_done > 0 {
                line.push_str(&format!("   {} declared done, then failed verification", c.false_done));
            }
            if c.errors > 0 {
                line.push_str(&format!("   {} lost to provider errors", plural(c.errors, "run")));
            }
            out.push(line);
        }

        for variant in &r.variants {
            // Presage's variants are measured against classic, another harness against Presage.
            let against = if variant.starts_with("ext:") && r.variants.iter().any(|v| v == "presage") { "presage" } else { "classic" };
            let (Some(c), Some(base)) = (cells(ALL, variant), cells(ALL, against)) else { continue };
            if variant == against {
                continue;
            }
            let (a, b) = (&c.median, &base.median);
            out.push(format!(
                "{variant} against {against}: turns {}, input tokens {}, output tokens {}, cost {} ({} with no cache), time {}; solved {}/{} against {}/{}",
                delta(a.turns, b.turns),
                delta(a.prompt_tokens, b.prompt_tokens),
                delta(a.output_tokens, b.output_tokens),
                delta(a.cost, b.cost),
                delta(a.cost_uncached, b.cost_uncached),
                delta(a.elapsed_ms, b.elapsed_ms),
                c.solved,
                c.runs,
                base.solved,
                base.runs
            ));
            let pairs: Vec<(&Cell, &Cell)> = r.tasks.iter().filter_map(|t| Some((cells(t, variant)?, cells(t, against)?))).collect();
            let count = |better: fn(&Medians, &Medians) -> bool| pairs.iter().filter(|(x, y)| better(&x.median, &y.median)).count();
            out.push(format!(
                "  by task ({}): fewer turns on {}, more on {}; cheaper on {}, dearer on {} (with no cache: {} and {})",
                pairs.len(),
                count(|x, y| x.turns < y.turns),
                count(|x, y| x.turns > y.turns),
                count(|x, y| x.cost < y.cost),
                count(|x, y| x.cost > y.cost),
                count(|x, y| x.cost_uncached < y.cost_uncached),
                count(|x, y| x.cost_uncached > y.cost_uncached)
            ));
        }
    }

    let unsolved: Vec<&Run> = r.runs.iter().filter(|run| run.error.is_some() || !run.solved).collect();
    if !unsolved.is_empty() {
        out.push(String::new());
        out.push(format!("{} without a solution:", plural(unsolved.len(), "run")));
        for run in unsolved.iter().take(12) {
            let why = match (&run.error, &run.failure) {
                (Some(e), _) => format!("no result: {}", one_line(e, 110)),
                (None, Some(f)) => format!("{}: {}", run.outcome, one_line(f, 110)),
                (None, None) => format!("{}; verification: {}", run.outcome, one_line(run.verify_tail.lines().last().unwrap_or(""), 110)),
            };
            out.push(format!("  {} · {} · {} #{} · session {} · {why}", run.model, run.task, run.variant, run.repeat, if run.session.is_empty() { "–" } else { &run.session }));
        }
        if unsolved.len() > 12 {
            out.push(format!("  … and {} more, all in results.json", unsolved.len() - 12));
        }
    }
    out.push(String::new());
    out.push(format!("results: {}/results.json", r.dir));
    if !r.prefix_tokens.is_empty() {
        out.push(format!("replay a run: PRESAGE_HOME={}/home presage show <session>", r.dir));
    }
    if !r.first_prompt_tokens.is_empty() {
        out.push(format!("output of an external run: {}/external/<session>/output.log", r.dir));
    }
    out.join("\n")
}

pub async fn command(h: &Harness, args: &[String]) -> ExitCode {
    let fail = |msg: String| {
        eprintln!("error: {msg}");
        ExitCode::from(2)
    };
    match parse(args) {
        Err(e) => fail(e),
        Ok(Command::Help) => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        Ok(Command::List) => {
            for (set, tasks) in tasks::SETS {
                for t in tasks.iter() {
                    println!("{:<6} {:<12} {}", set, t.id, t.title);
                }
            }
            ExitCode::SUCCESS
        }
        Ok(Command::Check(tasks)) => {
            let mut bad = 0;
            for (id, verdict) in check(&tasks).await {
                match verdict {
                    Ok(()) => println!("ok   {id}: fails as shipped, passes with its reference solution"),
                    Err(e) => {
                        bad += 1;
                        println!("BAD  {id}: {e}");
                    }
                }
            }
            if bad == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) }
        }
        Ok(Command::Run(opt)) => match execute(h, &opt, true).await {
            Err(e) => fail(e),
            Ok(report) => {
                println!("{}", render(&report));
                if let Some(path) = &opt.out {
                    let copied = serde_json::to_string_pretty(&report).map_err(|e| e.to_string()).and_then(|body| std::fs::write(path, body).map_err(|e| e.to_string()));
                    if let Err(e) = copied {
                        return fail(format!("could not write {}: {e}", path.display()));
                    }
                }
                ExitCode::SUCCESS
            }
        },
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn harness(name: &str) -> Harness {
        Harness::at(std::env::temp_dir().join(format!("presage-bench-{name}-{}", rand_hex(4))))
    }

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    #[tokio::test]
    async fn every_task_fails_as_shipped_and_passes_with_its_reference_solution() {
        if !on_path("python3") {
            eprintln!("skipped: python3 is not installed");
            return;
        }
        let all: Vec<&'static Task> = TASKS.iter().collect();
        for (id, verdict) in check(&all).await {
            assert_eq!(verdict, Ok(()), "task {id}");
        }
    }

    #[tokio::test]
    async fn a_scripted_benchmark_runs_verifies_and_summarises() {
        if !on_path("python3") {
            eprintln!("skipped: python3 is not installed");
            return;
        }
        let h = harness("scripted");
        let Ok(Command::Run(opt)) = parse(&args("--scripted --tasks pager,report --variants presage,classic,no-auto-watch -n 2 -j 3")) else { panic!("should parse") };
        let report = execute(&h, &opt, false).await.unwrap();
        assert!(report.simulated);
        assert_eq!(report.runs.len(), 12);
        assert!(report.runs.iter().all(|r| r.solved && r.error.is_none() && r.outcome == "done"), "{:?}", report.runs);
        // Presage vouches for what it declares done; the baseline only claims.
        assert!(report.runs.iter().all(|r| r.harness_verified == (r.variant != "classic")));
        // Runs come back in matrix order however they finished.
        assert_eq!(report.runs.iter().take(3).map(|r| r.variant.as_str()).collect::<Vec<_>>(), ["presage", "classic", "no-auto-watch"]);

        let all = report.summary.iter().find(|c| c.task == ALL && c.variant == "presage").unwrap();
        assert_eq!((all.runs, all.solved, all.false_done, all.errors, all.median.turns), (4, 4, 0, 0, 2.0));
        let text = render(&report);
        assert!(text.contains("SCRIPTED STAND-IN") && text.contains("no-auto-watch against classic"), "{text}");

        let dir = PathBuf::from(&report.dir);
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("results.json")).unwrap()).unwrap();
        assert_eq!(stored["runs"].as_array().unwrap().len(), 12);
        assert_eq!(std::fs::read_dir(dir.join("work")).unwrap().count(), 0, "workspaces are removed unless --keep");
        // Each run's log is kept and can be replayed from the benchmark's own data directory.
        assert_eq!(h.with_data_dir(dir.join("home")).list().len(), 12);
    }

    /// The workspace of a fresh run of `task` and the first message a model receives in it.
    async fn opening(h: &Harness, task: &Task, without: &[&str]) -> (String, String) {
        let ws = std::env::temp_dir().join(format!("presage-bench-opening-{}", rand_hex(4)));
        materialize(&ws, task.files).unwrap();
        let received = Arc::new(Mutex::new(String::new()));
        let keep = received.clone();
        let turn: presage_core::provider::mock::TurnFn = Box::new(move |messages| {
            *keep.lock().unwrap() = messages[1].content.clone();
            MockTurn::say("Looking into it.")
        });
        let req = StartRequest { cwd: ws.to_string_lossy().into_owned(), task: task.prompt.to_string(), without: without.iter().map(|w| w.to_string()).collect(), ..Default::default() };
        let meta = h.start_scripted(req, MockProvider::new(vec![turn])).unwrap();
        wait_for_end(h, &meta.id).await.unwrap();
        let root = std::fs::canonicalize(&ws).unwrap().to_string_lossy().into_owned();
        let _ = std::fs::remove_dir_all(&ws);
        let first = received.lock().unwrap().clone();
        (root, first)
    }

    #[tokio::test]
    async fn a_run_opens_with_the_file_the_task_names_and_its_failing_tests() {
        if !on_path("python3") {
            eprintln!("skipped: python3 is not installed");
            return;
        }
        let h = harness("opening");
        let task = TASKS.iter().find(|t| t.id == "pager").unwrap();

        let (root, first) = opening(&h, task, &[]).await;
        assert!(first.contains("\n</env>\n\n<files>\npager.py · 12 lines\n") && first.contains("\n</files>\nThis is `read` output for the file the task names."), "{first}");
        assert!(first.contains("\n\n<tests>\nwatch tests: FAIL (expected exit 0, got exit 1)\n") && first.ends_with(&format!("\n\n<task>\n{}\n</task>", task.prompt)), "{first}");
        // The traceback names its file the way a tool call would, and still brings the lines along.
        assert!(first.contains("\n  File \"test_pager.py\", line 12, in test_first_page\n") && first.contains("\n--- where (current file content; anchors work in edit) ---\ntest_pager.py\n"), "{first}");
        assert_eq!(first.matches(&root).count(), 1, "only the `cwd:` line names the workspace: {first}");
        assert_eq!(first.matches("|def total_pages(count, per_page):").count(), 1, "{first}");

        // Each of the two mechanisms can be taken out on its own.
        let (_, plain) = opening(&h, task, &["attach"]).await;
        assert!(!plain.contains("<files>") && !plain.contains("|def total_pages") && plain.contains("\n  File \"test_pager.py\", line 12, in test_first_page\n"), "{plain}");
        let (root, long) = opening(&h, task, &["short-paths"]).await;
        assert!(long.contains("\n<files>\npager.py · 12 lines\n") && long.contains(&format!("\n  File \"{root}/test_pager.py\", line 12, in test_first_page\n")), "{long}");
        assert!(long.contains("\n--- where (current file content; anchors work in edit) ---\ntest_pager.py\n"), "{long}");
    }

    #[tokio::test]
    async fn a_wrong_solution_is_not_counted_however_it_was_declared() {
        if !on_path("python3") {
            return;
        }
        let task = TASKS.iter().find(|t| t.id == "pager").unwrap();
        let ws = std::env::temp_dir().join(format!("presage-bench-cheat-{}", rand_hex(4)));
        materialize(&ws, task.files).unwrap();
        // Rewriting the visible test so that it passes does not help: it is put back first.
        std::fs::write(ws.join("test_pager.py"), "import unittest\n").unwrap();
        let (solved, tail) = verify(task, &ws).await;
        assert!(!solved && tail.contains("FAILED"), "{tail}");
        // Passing the visible tests alone is not enough either: the held-out ones run too.
        std::fs::write(ws.join("pager.py"), "def total_pages(count, per_page):\n    return 3 if count == 11 else 2\n\n\ndef page(items, number, per_page):\n    return items[(number - 1) * per_page:number * per_page]\n").unwrap();
        assert!(!verify(task, &ws).await.0);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn options_and_variants_parse() {
        let Ok(Command::Run(opt)) = parse(&args("-p logfare -m glm-5.3,glm-5.3-flash -m other --variants classic,no-receipts -n 5 -j 2 --max-cost 1.5 --keep")) else { panic!("should parse") };
        assert_eq!(opt.models, ["glm-5.3", "glm-5.3-flash", "other"]);
        assert_eq!((opt.repeats, opt.jobs, opt.max_cost, opt.keep, opt.tasks.len()), (5, 2, Some(1.5), true, TASKS.len()));
        assert_eq!(opt.variants[1], Variant::own("no-receipts", false, vec!["receipts".into()]));
        assert!(!opt.variants[1].features().receipts && opt.variants[1].features().scripts);
        // Every mechanism has its variant, however its name is spelled.
        let Ok(Command::Run(opt)) = parse(&args("--variants presage,no-attach,no-short-paths,no-short_paths")) else { panic!("should parse") };
        assert_eq!((opt.variants[1].without.as_slice(), opt.variants[2].without.as_slice(), opt.variants[3].without.as_slice()), (&["attach".to_string()][..], &["short_paths".to_string()][..], &["short_paths".to_string()][..]));
        assert!(!opt.variants[1].features().attach && opt.variants[1].features().short_paths && !opt.variants[2].features().short_paths && opt.variants[2].features().attach);

        assert!(matches!(parse(&args("--list")), Ok(Command::List)));
        assert!(matches!(parse(&args("--check --tasks roman")), Ok(Command::Check(t)) if t.len() == 1));
        for bad in ["--variants turbo", "--variants no-telepathy", "--tasks nope", "-n 0", "-n many", "--frobnicate", "-m"] {
            assert!(parse(&args(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn external_variants_parse() {
        let Ok(Command::Run(opt)) = parse(&args("--variants presage,ext:mini,EXT:Open-Code_2 --external-dir /tmp/adapters")) else { panic!("should parse") };
        let externals: Vec<(&str, Option<&str>)> = opt.variants.iter().map(|v| (v.name.as_str(), v.external.as_deref())).collect();
        assert_eq!(externals, [("presage", None), ("ext:mini", Some("mini")), ("ext:open-code_2", Some("open-code_2"))]);
        assert!(!opt.variants[1].classic && opt.variants[1].without.is_empty());
        assert_eq!(opt.external_dir, Some(PathBuf::from("/tmp/adapters")));
        assert_eq!(Options::default().external_dir, None);
        for bad in ["--variants ext:", "--variants ext:../mini", "--variants ext:a/b", "--variants external:mini", "--external-dir"] {
            assert!(parse(&args(bad)).is_err(), "{bad}");
        }
    }

    fn adapter(dir: &Path, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        // `ask <json>` sends one chat completion to the endpoint the runner handed over.
        let ask = r#"ask() { python3 -c 'import os, sys, urllib.request as u
r = u.Request(os.environ["BENCH_BASE_URL"] + "/chat/completions", data=sys.argv[1].encode(), headers={"content-type": "application/json", "authorization": "Bearer " + os.environ["BENCH_API_KEY"]})
u.urlopen(r).read()' "$1"; }"#;
        let path = dir.join(name).join("run.sh");
        write_file(dir, &format!("{name}/run.sh"), &format!("#!/bin/sh\n{ask}\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[tokio::test]
    async fn an_external_harness_is_run_metered_and_verified_like_presage() {
        if !on_path("python3") {
            eprintln!("skipped: python3 is not installed");
            return;
        }
        let (upstream, seen) = proxy::fake::provider().await;
        let root = std::env::temp_dir().join(format!("presage-bench-external-{}", rand_hex(4)));
        let adapters = root.join("adapters");
        let roman = tasks::find("roman").unwrap();
        materialize(&root.join("solution"), roman.solution).unwrap();
        let solve = format!("cp {}/solution/roman.py . && touch \"$BENCH_SCRATCH/state\"", root.display());
        adapter(&adapters, "solver", &format!("ask '{{\"model\":\"'\"$BENCH_MODEL\"'\",\"messages\":[]}}' && ask '{{\"model\":\"'\"$BENCH_MODEL\"'\",\"stream\":true,\"messages\":[]}}'; {solve}"));
        adapter(&adapters, "idle", "echo 'no key configured' >&2");
        adapter(&adapters, "looper", "while ask '{\"model\":\"'\"$BENCH_MODEL\"'\",\"messages\":[]}'; do :; done 2>/dev/null; echo gave up; exit 1");
        write_file(&root, "home/config.json", &json!({"keys": {"custom": "real-key"}, "custom": {"base_url": upstream, "models": ["deepseek-chat"]}}).to_string()).unwrap();
        let h = Harness::at(root.join("home"));

        let line = format!("-p custom -m deepseek-chat,broken --tasks roman --variants ext:solver,ext:idle,ext:looper -n 1 -j 3 --max-turns 3 --retries 0 --external-dir {}", adapters.display());
        let Ok(Command::Run(opt)) = parse(&args(&line)) else { panic!("should parse") };
        let report = execute(&h, &opt, false).await.unwrap();
        let run = |model: &str, variant: &str| report.runs.iter().find(|r| r.model == model && r.variant == variant).unwrap();

        // Two requests, one of them a stream that did not ask for its usage.
        let solver = run("deepseek-chat", "ext:solver");
        let detail = solver.external.as_ref().unwrap();
        assert!(solver.solved && solver.error.is_none() && solver.failure.is_none(), "{solver:?}");
        assert_eq!((solver.outcome.as_str(), detail.exit_code, detail.usage.usage_injected, detail.usage.unmetered), ("done", Some(0), 1, 0));
        let s = &solver.stats;
        assert_eq!((s.turns, s.prompt_tokens, s.cached_tokens, s.output_tokens, s.reasoning_tokens, s.context_tokens), (2, 2100, 1800, 80, 20, 900));
        assert!(s.priced && s.cost > 0.0 && solver.cost_uncached > s.cost && s.elapsed_ms > 0);
        assert!(Path::new(&detail.log).is_file() && detail.log.ends_with(&format!("external/{}/output.log", solver.session)));
        assert!(!Path::new(&report.dir).join("external").join(&solver.session).join("scratch").exists(), "the scratch directory goes with the run");
        assert!(std::fs::read_dir(Path::new(&report.dir).join("work")).unwrap().next().is_none(), "workspaces are removed");

        let idle = run("deepseek-chat", "ext:idle");
        assert_eq!((idle.solved, idle.outcome.as_str(), idle.failure.as_deref(), idle.stats.turns), (false, "error", Some("made no model request: no key configured"), 0));
        assert!(idle.error.is_none() && idle.verify_tail.contains("roman"), "an adapter that does nothing is judged, and fails: {idle:?}");

        let looper = run("deepseek-chat", "ext:looper");
        assert_eq!((looper.solved, looper.outcome.as_str(), looper.failure.as_deref(), looper.stats.turns), (false, "budget", Some("stopped at the limit of 3 model requests"), 3));
        assert!(looper.external.as_ref().unwrap().usage.refused >= 1);

        // A provider that fails leaves no result, as for Presage's own runs.
        let lost = run("broken", "ext:solver");
        assert!(lost.error.as_deref().is_some_and(provider_failed) && !lost.solved, "{lost:?}");
        let all = |variant: &str| report.summary.iter().find(|c| c.model == "deepseek-chat" && c.task == ALL && c.variant == variant).unwrap();
        assert_eq!((all("ext:solver").solved, all("ext:solver").runs, all("ext:idle").solved, all("ext:idle").runs), (1, 1, 0, 1));
        assert_eq!(report.summary.iter().find(|c| c.model == "broken" && c.task == ALL && c.variant == "ext:solver").map(|c| (c.runs, c.errors)), Some((0, 1)));

        let seen = seen.lock().unwrap().clone();
        assert!(!seen.is_empty() && seen.iter().all(|(_, _, auth, _)| auth == "Bearer real-key"), "{seen:?}");
        assert!(report.prefix_tokens.is_empty());
        assert_eq!(report.first_prompt_tokens.iter().map(|p| (p.variant.as_str(), p.tokens)).collect::<Vec<_>>(), [("ext:solver", 1200), ("ext:idle", 0), ("ext:looper", 1200)]);
        let text = render(&report);
        for expected in ["first prompt with the task, measured: ext:solver ~1200 tokens", "ext:idle", "made no model request: no key configured", "stopped at the limit of 3 model requests", "no result: provider returned HTTP 503", "/external/<session>/output.log"] {
            assert!(text.contains(expected), "{expected:?} missing from:\n{text}");
        }

        let Ok(Command::Run(scripted)) = parse(&args(&format!("{line} --scripted"))) else { panic!("should parse") };
        assert!(execute(&h, &scripted, false).await.is_err_and(|e| e.contains("needs a real model")));
        let Ok(Command::Run(unknown)) = parse(&args(&line.replace("ext:idle", "ext:nope"))) else { panic!("should parse") };
        assert!(execute(&h, &unknown, false).await.is_err_and(|e| e.contains("no adapter for ext:nope")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn medians_ignore_runs_the_provider_lost() {
        let run = |turns: u32, cost: f64, solved: bool, error: Option<&str>| Run {
            seq: 0,
            model: "m".into(),
            task: "t".into(),
            variant: "presage".into(),
            repeat: 1,
            session: String::new(),
            outcome: "done".into(),
            harness_verified: solved,
            solved,
            failure: None,
            verify_tail: String::new(),
            error: error.map(String::from),
            cost_uncached: cost * 2.0,
            stats: Stats { turns, cost, ..Stats::default() },
            external: None,
        };
        let runs = [run(3, 0.01, true, None), run(5, 0.03, false, None), run(4, 0.02, true, None), run(0, 0.0, false, Some("provider returned HTTP 502: overloaded"))];
        let c = cell("m", "t", "presage", &runs.iter().collect::<Vec<_>>());
        assert_eq!((c.runs, c.errors, c.solved, c.false_done), (3, 1, 2, 1));
        assert_eq!((c.median.turns, c.median.cost, c.median.cost_uncached), (4.0, 0.02, 0.04));
        assert!((c.total_cost - 0.06).abs() < 1e-12);
        assert_eq!(median(vec![1.0, 9.0, 2.0, 4.0]), 3.0);
        assert_eq!((delta(3.0, 5.0), delta(1.0, 0.0), num(2.5), num(3.0)), ("-40%".into(), "n/a".into(), "2.5".into(), "3".into()));
        assert!(provider_failed("provider returned HTTP 429: slow down") && !provider_failed("the model returned neither a tool call nor an answer three times in a row"));
    }
}
