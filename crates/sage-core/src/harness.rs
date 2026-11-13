//! The public facade: everything a front end (desktop app, CLI) needs, and nothing else.
//!
//! A session is a long-lived task that owns its agent. Events are appended to `events.jsonl`,
//! kept in memory for late subscribers, and broadcast live.

use crate::agent::{Agent, AgentConfig, EffortPolicy, Retune};
use crate::config::{CUSTOM, Config, PROVIDERS, Resolved, data_dir};
use crate::demo;
use crate::economy::prices_for;
use crate::events::{Event, EventKind, FinishStatus, GoalState, Sink, Stats};
use crate::goal::{self, Goal};
use crate::prompt;
use crate::provider::Provider;
use crate::provider::mock::MockProvider;
use crate::provider::openai::OpenAiProvider;
use crate::resume::{self, SessionLock, Setup};
use crate::script::{Approvals, Autonomy};
use crate::skills::{self, SkillInfo};
use crate::state::{Features, Mode, Session};
use crate::util::{civil_date, home_dir, now_ms, one_line, rand_hex};
use crate::workspace::Workspace;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Running,
    /// Waiting for the next message; the agent and its context are still alive.
    Idle,
    /// Loaded from disk: viewable, not resumable (`SessionMeta::why_closed` says why).
    Closed,
    /// Stored by a program that has ended and not running in this one: sending it a message
    /// continues it, from the snapshot it kept. Only `list` and `meta` say this.
    Resumable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub provider: String,
    pub model: String,
    pub mode: String,
    pub effort: String,
    pub simulated: bool,
    pub created: u64,
    pub updated: u64,
    pub status: SessionStatus,
    /// How the most recent run ended, once it has.
    pub outcome: Option<FinishStatus>,
    pub verified: Option<bool>,
    pub stats: Stats,
    /// Names of the skills switched on, in the order they were.
    #[serde(default)]
    pub skills: Vec<String>,
    /// The session's goal (`/goal`), and whether a check has found it met.
    #[serde(default)]
    pub goal: Option<Goal>,
    /// For a stored session that cannot be continued, in a word why: `older` (stored before
    /// sessions kept snapshots), `simulated` (played by a stand-in), `busy` (open in another
    /// process) or `gone` (its folder is). Worked out when listed, never stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why_closed: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct StartRequest {
    pub cwd: String,
    pub task: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// `low`, `high`, `max`, `off`, or `ratchet[-low|-high]`.
    #[serde(default)]
    pub effort: Option<String>,
    /// `ecosage` (default), `default` or `prosage`, the conventional baseline loop.
    #[serde(default)]
    pub mode: Option<String>,
    /// `auto` (default) or `ask`.
    #[serde(default)]
    pub autonomy: Option<String>,
    #[serde(default)]
    pub max_turns: Option<u32>,
    #[serde(default)]
    pub time_budget_secs: Option<u64>,
    /// True when a person is present to answer questions and approvals.
    #[serde(default)]
    pub interactive: bool,
    /// Mechanisms to switch off, for ablation runs (see `Features::NAMES`).
    #[serde(default)]
    pub without: Vec<String>,
    /// Mechanisms to switch on that are off unless asked for (see `Features::OPTIONAL`).
    #[serde(default)]
    pub with: Vec<String>,
    /// A system prompt template for this run alone, in place of the user's or the built-in one.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Skills to switch on, by name.
    #[serde(default)]
    pub skills: Vec<String>,
    /// Files attached to the task, by path.
    #[serde(default)]
    pub files: Vec<String>,
    /// `false` leaves the project's instruction file (`AGENTS.md`, …) out of the system prompt.
    #[serde(default)]
    pub project_instructions: Option<bool>,
    /// A condition to work until it holds, as `/goal <condition>` sets one. With no task, it is the
    /// task. A task that itself starts with `/goal` sets the goal, and this one is not used.
    #[serde(default)]
    pub goal: Option<String>,
}

/// What a message may change about the session it is sent to, as `StartRequest` names each.
/// Left out, a thing stays as it is.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Changes {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub autonomy: Option<String>,
}

/// The files a message may carry: each one a file that is there, and each once.
fn attachable(files: &[String]) -> Result<Vec<String>, String> {
    let mut kept: Vec<String> = Vec::new();
    for file in files.iter().map(|file| file.trim()).filter(|file| !file.is_empty()) {
        if !Path::new(file).is_file() {
            return Err(format!("cannot attach {file}: there is no such file"));
        }
        if !kept.iter().any(|each| each == file) {
            kept.push(file.to_string());
        }
    }
    Ok(kept)
}

/// What the user can edit of the system prompt.
#[derive(Clone, Debug, Serialize)]
pub struct Instructions {
    /// The template in effect: the user's own, or else the built-in one.
    pub system: String,
    pub system_default: String,
    pub system_custom: bool,
    /// The user's instructions for every project (`<data_dir>/AGENTS.md`).
    pub global: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    /// What it is known for, in a word or two. Empty where the name says it.
    pub serves: String,
    pub base_url: String,
    pub key_env: String,
    pub has_key: bool,
    pub models: Vec<String>,
    pub note: String,
    /// An endpoint of the user's own: it asks for no key and can be taken away.
    pub custom: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct AppState {
    pub version: String,
    pub providers: Vec<ProviderInfo>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub effort: String,
    /// Whether a provider that is rate limited or unreachable is retried until it answers.
    pub auto_retry: bool,
    pub data_dir: String,
    pub home: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DirListing {
    pub path: String,
    pub parent: Option<String>,
    pub dirs: Vec<DirEntry>,
    pub is_git: bool,
}

struct Live {
    dir: PathBuf,
    meta: Mutex<SessionMeta>,
    events: Mutex<Vec<Event>>,
    seq: AtomicU64,
    tx: broadcast::Sender<Event>,
    log: Mutex<Option<std::fs::File>>,
    input: mpsc::UnboundedSender<Input>,
    cancel: Mutex<CancellationToken>,
    /// Cancelled when the session is closed for good.
    closed: CancellationToken,
    approvals: Arc<Approvals>,
    /// This process's claim on the session's directory, let go of when the session is closed.
    lock: Mutex<Option<SessionLock>>,
}

/// What the worker of a session is sent.
enum Input {
    /// A user message to work on, the skills to switch on with it, and what else changes with it.
    Message {
        /// What the model is sent.
        text: String,
        /// What the log shows, where the user typed something else.
        shown: Option<String>,
        skills: Vec<SkillInfo>,
        retune: Retune,
        files: Vec<String>,
    },
    /// `/goal` or `/goal clear`: answered by the session itself, with no request to the model.
    Goal { shown: String, command: goal::Command, retune: Retune },
}

impl Live {
    fn save_meta(&self) {
        let meta = self.meta.lock().unwrap().clone();
        if let Ok(body) = serde_json::to_string_pretty(&meta) {
            let _ = std::fs::write(self.dir.join("meta.json"), body);
        }
    }

    fn set_status(&self, status: SessionStatus) {
        {
            let mut m = self.meta.lock().unwrap();
            m.status = status;
            m.updated = now_ms();
        }
        self.save_meta();
    }

    /// Ends the run in progress, if any, and lets another process have the session.
    fn shut(&self) {
        self.cancel.lock().unwrap().cancel();
        self.closed.cancel();
        self.lock.lock().unwrap().take();
    }
}

impl Sink for Live {
    fn emit(&self, kind: EventKind) {
        let transient = kind.transient();
        // Holding the backlog lock across the broadcast lets `subscribe` take a snapshot and a
        // receiver with nothing lost or duplicated in between.
        let mut backlog = self.events.lock().unwrap();
        let event = Event { seq: self.seq.fetch_add(1, Ordering::SeqCst) + 1, ts: now_ms(), kind };
        if !transient {
            if let (Some(file), Ok(line)) = (self.log.lock().unwrap().as_mut(), serde_json::to_string(&event)) {
                let _ = writeln!(file, "{line}");
            }
            match &event.kind {
                EventKind::Stats { stats } => self.meta.lock().unwrap().stats = stats.clone(),
                EventKind::Finish { status, verified, .. } => {
                    let mut m = self.meta.lock().unwrap();
                    m.outcome = Some(*status);
                    m.verified = Some(*verified);
                }
                EventKind::Goal { state: state @ (GoalState::Set | GoalState::Cleared | GoalState::Met), text, .. } => {
                    {
                        let mut m = self.meta.lock().unwrap();
                        match state {
                            GoalState::Set => m.goal = Some(Goal::new(text)),
                            GoalState::Cleared => m.goal = None,
                            _ => {
                                if let Some(goal) = m.goal.as_mut() {
                                    goal.met = true;
                                }
                            }
                        }
                    }
                    // A goal that has been met is kept as met even if the app is closed in the middle of the run.
                    self.save_meta();
                }
                _ => {}
            }
            backlog.push(event.clone());
        }
        let _ = self.tx.send(event);
    }
}

struct Inner {
    data_dir: PathBuf,
    config: Mutex<Config>,
    sessions: Mutex<HashMap<String, Arc<Live>>>,
    /// The auto-retry setting, shared with the provider of every session: switching it reaches
    /// the ones that are running, even one that is waiting between two attempts.
    auto_retry: Arc<AtomicBool>,
    /// Sessions that a stand-in played are listed as continuable (see `resume_stand_ins`).
    stand_ins: AtomicBool,
}

impl Drop for Inner {
    /// The last handle to a harness going away ends its sessions, and frees them for whoever
    /// opens the data directory next: a session's worker would otherwise outlive it, lock and all.
    fn drop(&mut self) {
        for live in self.sessions.get_mut().map(|sessions| sessions.drain().map(|(_, live)| live).collect::<Vec<_>>()).unwrap_or_default() {
            live.shut();
        }
    }
}

#[derive(Clone)]
pub struct Harness {
    inner: Arc<Inner>,
}

fn session_id() -> String {
    let secs = now_ms() / 1000;
    let (y, m, d) = civil_date(secs);
    let t = secs % 86_400;
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}-{}", t / 3600, (t % 3600) / 60, t % 60, rand_hex(2))
}

/// The mode a request asks for and the mechanisms it switches on.
fn features_of(req: &StartRequest) -> Result<(Features, Mode), String> {
    let mode = match req.mode.as_deref() {
        Some(name) => Mode::parse(name).ok_or_else(|| format!("unknown mode {name:?}; use {}", Mode::NAMES.join(", ")))?,
        None => Mode::default(),
    };
    let mut features = mode.features();
    let unknown = |name: &String| format!("unknown mechanism {name:?}; known: {}", Features::NAMES.join(", "));
    for name in &req.with {
        features = features.with(name).ok_or_else(|| unknown(name))?;
    }
    for name in &req.without {
        features = features.without(name).ok_or_else(|| unknown(name))?;
    }
    Ok((features, mode))
}

/// What the first message of a session comes to.
struct Opening {
    /// What the model is sent.
    task: String,
    /// What the log shows, where the user typed something else (`/goal <condition>`).
    shown: Option<String>,
    goal: Option<String>,
    title: String,
}

/// A task that is `/goal <condition>` sets the goal and is worded as a task; a goal given beside a
/// task is told to the model after it; a goal with no task is the task. A first message cannot be
/// `/goal` alone or `/goal clear`: there is no goal to report or clear yet.
fn opening_of(req: &StartRequest) -> Result<Opening, String> {
    let task = req.task.trim();
    match goal::parse_command(task) {
        Some(goal::Command::Set(condition)) => return Ok(Opening { task: goal::task(&condition), shown: Some(task.to_string()), title: one_line(&condition, 72), goal: Some(condition) }),
        Some(_) => return Err("a session cannot start with `/goal` alone or `/goal clear`: there is no goal yet. Say the condition: /goal <condition>".into()),
        None => {}
    }
    let title = one_line(task, 72);
    match req.goal.as_deref().map(str::trim) {
        Some("") => Err("the goal is empty".into()),
        Some(condition) if task.is_empty() => Ok(Opening { task: goal::task(condition), shown: None, title: one_line(condition, 72), goal: Some(condition.to_string()) }),
        Some(condition) => Ok(Opening { task: format!("{task}\n\n{}", goal::briefing(condition)), shown: Some(task.to_string()), title, goal: Some(condition.to_string()) }),
        None => Ok(Opening { task: task.to_string(), shown: None, title, goal: None }),
    }
}

/// Runs a session: takes its messages one at a time and works on each until the session is closed.
fn spawn_worker(worker: Arc<Live>, mut agent: Agent, mut inbox: mpsc::UnboundedReceiver<Input>) {
    tokio::spawn(async move {
        loop {
            // A session that is closed takes no more messages, one that was already waiting included.
            let message = tokio::select! {
                biased;
                _ = worker.closed.cancelled() => None,
                message = inbox.recv() => message,
            };
            let Some(input) = message else { break };
            let cancel = CancellationToken::new();
            *worker.cancel.lock().unwrap() = cancel.clone();
            // Closed just before the line above: `shut` cancelled the token this one replaced.
            if worker.closed.is_cancelled() {
                break;
            }
            worker.set_status(SessionStatus::Running);
            match input {
                Input::Message { text, shown, skills, retune, files } => {
                    agent.retune(retune);
                    agent.attach(files);
                    if let Some(shown) = shown {
                        agent.shown_as(shown);
                    }
                    agent.run_with(text, &skills, &cancel).await;
                }
                Input::Goal { shown, command, retune } => {
                    agent.retune(retune);
                    agent.goal_command(&shown, command);
                }
            }
            worker.set_status(SessionStatus::Idle);
        }
        agent.sess.jobs.kill_all();
        worker.lock.lock().unwrap().take();
    });
}

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().filter(|t| !t.trim().is_empty())
}

/// Writes `text` to `path`; with nothing to write, the file goes.
fn store_text(path: &Path, text: &str) -> Result<(), String> {
    let stored = if text.trim().is_empty() {
        std::fs::remove_file(path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
    } else {
        path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(path, text))
    };
    stored.map_err(|e| format!("could not save {}: {e}", path.display()))
}

impl Harness {
    pub fn new() -> Self {
        Self::at(data_dir())
    }

    pub fn at(data_dir: PathBuf) -> Self {
        let config = Config::load(&data_dir.join("config.json"));
        let auto_retry = Arc::new(AtomicBool::new(config.auto_retry));
        Self { inner: Arc::new(Inner { data_dir, config: Mutex::new(config), sessions: Mutex::new(HashMap::new()), auto_retry, stand_ins: AtomicBool::new(false) }) }
    }

    /// A harness with this one's keys and defaults that keeps its sessions elsewhere,
    /// so that benchmark runs do not mix with the user's own. It never retries without end: a
    /// benchmark has to finish.
    pub fn with_data_dir(&self, data_dir: PathBuf) -> Self {
        let config = self.inner.config.lock().unwrap().clone();
        Self { inner: Arc::new(Inner { data_dir, config: Mutex::new(config), sessions: Mutex::new(HashMap::new()), auto_retry: Arc::default(), stand_ins: AtomicBool::new(false) }) }
    }

    fn config_path(&self) -> PathBuf {
        self.inner.data_dir.join("config.json")
    }

    pub fn state(&self) -> AppState {
        let cfg = self.inner.config.lock().unwrap();
        let mut providers: Vec<ProviderInfo> = PROVIDERS
            .iter()
            .map(|d| ProviderInfo {
                id: d.id.into(),
                name: d.name.into(),
                serves: d.serves.into(),
                base_url: d.base_url.into(),
                key_env: d.key_env.into(),
                has_key: cfg.has_key(d),
                models: d.models.iter().map(|m| m.to_string()).collect(),
                note: d.note.into(),
                custom: false,
            })
            .collect();
        providers.extend(cfg.customs().map(|(id, name, c)| ProviderInfo {
            id: id.into(),
            name: name.into(),
            serves: String::new(),
            base_url: c.base_url.clone(),
            key_env: if id == CUSTOM { "SAGE_API_KEY".into() } else { String::new() },
            has_key: true,
            models: c.models.clone(),
            note: String::new(),
            custom: true,
        }));
        let provider = cfg.default_provider();
        let model = cfg.resolve(provider.as_deref(), None).ok().map(|r| r.model);
        AppState {
            version: env!("CARGO_PKG_VERSION").into(),
            providers,
            provider,
            model,
            effort: cfg.effort.clone().unwrap_or_else(|| "ratchet".into()),
            auto_retry: self.inner.auto_retry.load(Ordering::Relaxed),
            data_dir: self.inner.data_dir.to_string_lossy().into_owned(),
            home: home_dir().to_string_lossy().into_owned(),
        }
    }

    /// Stores (or, with an empty key, removes) an API key.
    pub fn set_key(&self, provider: &str, key: &str) -> Result<(), String> {
        let mut cfg = self.inner.config.lock().unwrap();
        // The file's own endpoint may be given its key before it is written there.
        if provider != CUSTOM && !cfg.knows(provider) {
            return Err(format!("unknown provider {provider:?}"));
        }
        // A key removed is gone from the environment too, for Sage; one given turns it back on.
        if key.trim().is_empty() {
            cfg.keys.remove(provider);
            cfg.env_off.insert(provider.to_string());
        } else {
            cfg.keys.insert(provider.to_string(), key.trim().to_string());
            cfg.env_off.remove(provider);
        }
        cfg.save(&self.config_path()).map_err(|e| format!("could not save the configuration: {e}"))
    }

    /// Adds an OpenAI-compatible endpoint of the user's own, with its key when it asks for one,
    /// and gives the id it goes by.
    pub fn add_provider(&self, name: &str, base_url: &str, models: &[String], key: &str) -> Result<String, String> {
        let mut cfg = self.inner.config.lock().unwrap();
        let id = cfg.add_provider(name, base_url, models)?;
        if !key.trim().is_empty() {
            cfg.keys.insert(id.clone(), key.trim().to_string());
        }
        cfg.save(&self.config_path()).map_err(|e| format!("could not save the configuration: {e}"))?;
        Ok(id)
    }

    /// Takes one of the user's own endpoints away. Sessions already running on it keep running.
    pub fn remove_provider(&self, id: &str) -> Result<(), String> {
        let mut cfg = self.inner.config.lock().unwrap();
        cfg.remove_provider(id)?;
        cfg.save(&self.config_path()).map_err(|e| format!("could not save the configuration: {e}"))
    }

    pub fn set_defaults(&self, provider: Option<String>, model: Option<String>, effort: Option<String>) -> Result<(), String> {
        if let Some(e) = &effort {
            EffortPolicy::parse(e).ok_or_else(|| format!("unknown effort {e:?}; use low, high, max or ratchet"))?;
        }
        let mut cfg = self.inner.config.lock().unwrap();
        // A saved model is its provider's: with another provider and no model named, none is kept,
        // or the old provider's model would be asked of the new one.
        if model.is_none() && provider.as_ref().is_some_and(|next| cfg.provider.as_ref() != Some(next)) {
            cfg.model = None;
        }
        cfg.provider = provider.or(cfg.provider.take());
        cfg.model = model.or(cfg.model.take());
        cfg.effort = effort.or(cfg.effort.take());
        cfg.save(&self.config_path()).map_err(|e| format!("could not save the configuration: {e}"))
    }

    /// Switches auto-retry: with it, a request the provider turned away for a reason that passes
    /// (a rate limit, an overload, a dropped connection) is made again until it is answered or the
    /// session is stopped, instead of ending the run after a few attempts. Takes effect at once,
    /// in the sessions that are running too.
    pub fn set_auto_retry(&self, on: bool) -> Result<(), String> {
        let mut cfg = self.inner.config.lock().unwrap();
        cfg.auto_retry = on;
        cfg.save(&self.config_path()).map_err(|e| format!("could not save the configuration: {e}"))?;
        self.inner.auto_retry.store(on, Ordering::Relaxed);
        Ok(())
    }

    fn system_path(&self) -> PathBuf {
        self.inner.data_dir.join("system.md")
    }

    fn global_path(&self) -> PathBuf {
        self.inner.data_dir.join("AGENTS.md")
    }

    pub fn instructions(&self) -> Instructions {
        let custom = read_text(&self.system_path());
        let default = prompt::default_template().to_string();
        Instructions { system_custom: custom.is_some(), system: custom.unwrap_or_else(|| default.clone()), system_default: default, global: read_text(&self.global_path()).unwrap_or_default() }
    }

    /// Stores the user's system prompt template. `None`, nothing, or the built-in text itself goes
    /// back to the built-in one.
    pub fn set_system_prompt(&self, template: Option<&str>) -> Result<(), String> {
        let text = template.unwrap_or("");
        store_text(&self.system_path(), if text.trim() == prompt::default_template() { "" } else { text })
    }

    /// Stores (or, with nothing, removes) the user's instructions for every project.
    pub fn set_global_instructions(&self, text: &str) -> Result<(), String> {
        store_text(&self.global_path(), text)
    }

    /// The skills on offer, with the project's own where `cwd` is given.
    pub fn skills(&self, cwd: Option<&str>) -> Vec<SkillInfo> {
        skills::list(&self.inner.data_dir, cwd.map(str::trim).filter(|c| !c.is_empty()).map(Path::new))
    }

    /// The whole file of a skill, front matter included: what `save_skill` takes.
    pub fn skill_file(&self, name: &str, cwd: Option<&str>) -> Result<String, String> {
        skills::file_text(&self.inner.data_dir, cwd.map(str::trim).filter(|c| !c.is_empty()).map(Path::new), name)
    }

    /// Stores a skill of the user's. Under the name of a built-in one, it takes that one's place.
    pub fn save_skill(&self, name: &str, content: &str) -> Result<SkillInfo, String> {
        skills::save_user(&self.inner.data_dir, name, content)
    }

    /// Deletes a skill of the user's. Where it stood in for a built-in one, that one is back.
    pub fn delete_skill(&self, name: &str) -> Result<(), String> {
        skills::delete_user(&self.inner.data_dir, name)
    }

    /// The system prompt a session started with `req` would be given.
    pub fn system_prompt(&self, req: &StartRequest) -> Result<String, String> {
        let ws = Workspace::new(Path::new(req.cwd.trim())).map_err(|e| format!("cannot use {:?} as a workspace: {e}", req.cwd))?;
        let (features, mode) = features_of(req)?;
        self.compose_prompt(req, &ws.root, &features, mode == Mode::Plain).map(|(system, _)| system)
    }

    /// The system prompt and the skills of a session. The classic baseline has only its own prompt.
    fn compose_prompt(&self, req: &StartRequest, root: &Path, features: &Features, classic: bool) -> Result<(String, Vec<SkillInfo>), String> {
        let picked = skills::resolve(&self.inner.data_dir, Some(root), &req.skills)?;
        if classic {
            return Ok((prompt::SYSTEM_CLASSIC.to_string(), Vec::new()));
        }
        // With scripts switched off, a template about script turns would describe the wrong loop.
        let base = if features.scripts {
            let template = req.system_prompt.clone().filter(|t| !t.trim().is_empty()).or_else(|| read_text(&self.system_path()));
            prompt::render(template.as_deref().unwrap_or_else(|| prompt::default_template()), features)
        } else {
            prompt::SYSTEM_CLASSIC.to_string()
        };
        let global = read_text(&self.global_path()).unwrap_or_default();
        let project = if req.project_instructions.unwrap_or(true) { prompt::project_instructions(root) } else { None };
        Ok((prompt::compose(&base, &global, project.as_ref().map(|(file, text)| (*file, text.as_str())), &picked), picked))
    }

    /// Starts a session on a real provider and begins working on the task.
    pub fn start(&self, req: StartRequest) -> Result<SessionMeta, String> {
        if opening_of(&req)?.task.is_empty() {
            return Err("the task is empty".into());
        }
        let resolved = self.inner.config.lock().unwrap().resolve(req.provider.as_deref(), req.model.as_deref())?;
        let provider = self.client(&resolved)?;
        let effort = req.effort.clone().or_else(|| self.inner.config.lock().unwrap().effort.clone());
        self.launch(req, provider, &resolved.id, &resolved.model, effort)
    }

    /// The way to a model, retrying as the auto-retry setting says.
    fn client(&self, resolved: &Resolved) -> Result<Provider, String> {
        let mut client = OpenAiProvider::new(&resolved.base_url, &resolved.key, &resolved.model, resolved.dialect).map_err(|e| e.to_string())?;
        client.keep_retrying = self.inner.auto_retry.clone();
        Ok(Provider::OpenAi(client))
    }

    /// Starts the scripted walkthrough in a throwaway workspace. No key, no network.
    pub fn demo(&self) -> Result<SessionMeta, String> {
        let root = self.inner.data_dir.join("demo").join(session_id());
        demo::create_workspace(&root).map_err(|e| format!("could not create the demo workspace: {e}"))?;
        let real = crate::util::canonical(&root).map_err(|e| e.to_string())?;
        let provider = Provider::Mock(demo::provider(real, 22));
        // Its stand-in reads only the files it was written for: a survey would put others in front of it.
        let req = StartRequest { cwd: root.to_string_lossy().into_owned(), task: demo::TASK.into(), interactive: true, without: vec!["survey".into()], ..Default::default() };
        self.launch(req, provider, "demo", "scripted stand-in", Some("high".into()))
    }

    /// Starts a session whose model is a scripted stand-in, for tests and offline benchmark runs.
    /// Its numbers are fabricated and the session is marked as simulated.
    pub fn start_scripted(&self, req: StartRequest, provider: MockProvider) -> Result<SessionMeta, String> {
        let effort = req.effort.clone();
        self.launch(req, Provider::Mock(provider), "scripted", "scripted stand-in", effort)
    }

    fn launch(&self, req: StartRequest, provider: Provider, provider_id: &str, model: &str, effort: Option<String>) -> Result<SessionMeta, String> {
        let ws = Workspace::new(Path::new(req.cwd.trim())).map_err(|e| format!("cannot use {:?} as a workspace: {e}", req.cwd))?;
        let (features, mode) = features_of(&req)?;
        // Read once, here: the system prompt is the cached prefix of every request of the session.
        let (system, picked) = self.compose_prompt(&req, &ws.root, &features, mode == Mode::Plain)?;
        let skill_names: Vec<String> = picked.into_iter().map(|s| s.name).collect();
        let mode_label = if mode == Mode::Plain {
            mode.as_str().to_string()
        } else {
            let with = (!req.with.is_empty()).then(|| format!(" with {}", req.with.join(", ")));
            let without = (!req.without.is_empty()).then(|| format!(" without {}", req.without.join(", ")));
            format!("{}{}{}", mode.as_str(), with.unwrap_or_default(), without.unwrap_or_default())
        };
        let effort = match effort.as_deref() {
            Some(e) => EffortPolicy::parse(e).ok_or_else(|| format!("unknown effort {e:?}; use low, high, max or ratchet"))?,
            None => EffortPolicy::Ratchet(crate::provider::Effort::Low),
        };
        let autonomy = match req.autonomy.as_deref() {
            Some("ask") => Autonomy::Ask,
            Some("auto") | None => Autonomy::Auto,
            Some(other) => return Err(format!("unknown autonomy {other:?}; use auto or ask")),
        };
        let files = attachable(&req.files)?;
        let opening = opening_of(&req)?;

        let id = session_id();
        let dir = self.inner.data_dir.join("sessions").join(&id);
        let simulated = provider.simulated();
        let cwd = ws.root.to_string_lossy().into_owned();
        // Closing the folder above them covers the sessions stored before theirs were private.
        let _ = crate::util::private_dir(&self.inner.data_dir.join("sessions"));
        let sess = Session::new(ws, dir.clone(), features, prices_for(model)).map_err(|e| format!("could not create the session directory: {e}"))?;
        let lock = SessionLock::acquire(&dir).map_err(|e| format!("session {id}: {e}"))?;

        let mut cfg = AgentConfig::new(model);
        cfg.features = features;
        cfg.system = Some(system);
        cfg.effort = effort;
        cfg.autonomy = autonomy;
        cfg.interactive = req.interactive;
        if let Some(n) = req.max_turns {
            cfg.max_turns = n.max(1);
        }
        cfg.time_budget = req.time_budget_secs.filter(|s| *s > 0).map(Duration::from_secs);
        if features.routing {
            cfg.routable = skills::list(&self.inner.data_dir, Some(Path::new(&cwd))).into_iter().filter(|s| !skill_names.contains(&s.name)).collect();
        }

        let now = now_ms();
        let meta = SessionMeta {
            id: id.clone(),
            title: opening.title.clone(),
            cwd: cwd.clone(),
            provider: provider_id.to_string(),
            model: model.to_string(),
            mode: mode_label,
            effort: effort.label(),
            simulated,
            created: now,
            updated: now,
            status: SessionStatus::Running,
            outcome: None,
            verified: None,
            stats: Stats::default(),
            skills: skill_names.clone(),
            goal: opening.goal.as_deref().map(Goal::new),
            why_closed: None,
        };
        let (tx, _) = broadcast::channel(4096);
        let (input, inbox) = mpsc::unbounded_channel::<Input>();
        let approvals = Arc::new(Approvals::default());
        let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("events.jsonl")).ok();
        let live = Arc::new(Live { dir, meta: Mutex::new(meta.clone()), events: Mutex::new(Vec::new()), seq: AtomicU64::new(0), tx, log: Mutex::new(log), input, cancel: Mutex::new(CancellationToken::new()), closed: CancellationToken::new(), approvals: approvals.clone(), lock: Mutex::new(Some(lock)) });
        live.save_meta();
        live.emit(EventKind::SessionStart {
            id: id.clone(),
            title: meta.title.clone(),
            cwd,
            provider: meta.provider.clone(),
            model: meta.model.clone(),
            effort: meta.effort.clone(),
            mode: meta.mode.clone(),
            autonomy: if autonomy == Autonomy::Ask { "ask" } else { "auto" }.to_string(),
            simulated,
            skills: skill_names,
        });
        self.inner.sessions.lock().unwrap().insert(id, live.clone());

        let sink: Arc<dyn Sink> = live.clone();
        let mut agent = Agent::new(provider, sess, cfg, sink, approvals);
        agent.keep_snapshots(live.dir.join(resume::SNAPSHOT), Setup { mode: mode.as_str().to_string(), with: req.with.clone(), without: req.without.clone() });
        spawn_worker(live.clone(), agent, inbox);
        let retune = Retune { goal: opening.goal, ..Default::default() };
        live.input.send(Input::Message { text: opening.task, shown: opening.shown, skills: Vec::new(), retune, files }).map_err(|_| "the session ended before it started".to_string())?;
        Ok(meta)
    }

    fn live(&self, id: &str) -> Result<Arc<Live>, String> {
        self.inner.sessions.lock().unwrap().get(id).cloned().ok_or_else(|| format!("session {id} is not running in this process"))
    }

    /// Sends a follow-up message to an idle session, switching on the skills named that it does not
    /// have yet. Its system prompt is already sent, so they go in front of this message instead.
    pub fn send_with_skills(&self, id: &str, text: &str, skills: &[String]) -> Result<(), String> {
        self.send_with(id, text, skills, &[], Changes::default())
    }

    /// `send_with_skills`, and with the message whatever else is to change from it onwards: the
    /// model, the effort, the autonomy. The conversation is kept, so another model is sent all of
    /// it. The mode cannot change: the system prompt and the tools of a session are sent already.
    ///
    /// A message that starts with `/goal` is a command (see `goal`): `/goal <condition>` sets the
    /// session's goal and works towards it, `/goal` says what the goal is and `/goal clear` drops
    /// it, the last two with no request to the model.
    ///
    /// A session that is stored and not running in this process is brought back first (see
    /// `resume`), and the message goes to it as to any other.
    pub fn send_with(&self, id: &str, text: &str, skills: &[String], files: &[String], changes: Changes) -> Result<(), String> {
        let live = match self.live(id) {
            Ok(live) => live,
            Err(_) if text.trim().is_empty() => return Err("the message is empty".into()),
            Err(_) => self.revive(id, None, &changes)?,
        };
        if text.trim().is_empty() {
            return Err("the message is empty".into());
        }
        let command = goal::parse_command(text);
        let (cwd, simulated, provider, model) = {
            let meta = live.meta.lock().unwrap();
            (meta.cwd.clone(), meta.simulated, meta.provider.clone(), meta.model.clone())
        };
        let mut added = skills::resolve(&self.inner.data_dir, Some(Path::new(&cwd)), skills)?;
        let files = attachable(files)?;
        if matches!(command, Some(goal::Command::Show | goal::Command::Clear)) && (!added.is_empty() || !files.is_empty()) {
            return Err("a /goal command carries no skills or files: send them with a message".into());
        }
        let mut retune = Retune::default();
        if changes.provider.as_ref().is_some_and(|wanted| *wanted != provider) || changes.model.as_ref().is_some_and(|wanted| *wanted != model) {
            if simulated {
                return Err("this session is played by a stand-in: it has no model to change".into());
            }
            let resolved = self.inner.config.lock().unwrap().resolve(Some(changes.provider.as_deref().unwrap_or(&provider)), changes.model.as_deref())?;
            retune.provider = Some((self.client(&resolved)?, resolved.id, resolved.model));
        }
        if let Some(effort) = &changes.effort {
            retune.effort = Some(EffortPolicy::parse(effort).ok_or_else(|| format!("unknown effort {effort:?}; use low, high, max or ratchet"))?);
        }
        retune.autonomy = match changes.autonomy.as_deref() {
            Some("ask") => Some(Autonomy::Ask),
            Some("auto") => Some(Autonomy::Auto),
            Some(other) => return Err(format!("unknown autonomy {other:?}; use auto or ask")),
            None => None,
        };
        {
            let mut meta = live.meta.lock().unwrap();
            if meta.status == SessionStatus::Running {
                return Err("the session is still working; stop it first or wait".into());
            }
            // The classic baseline has no skills.
            added.retain(|s| Mode::parse(&meta.mode) != Some(Mode::Plain) && !meta.skills.contains(&s.name));
            meta.skills.extend(added.iter().map(|s| s.name.clone()));
            if let Some((_, provider, model)) = &retune.provider {
                meta.provider = provider.clone();
                meta.model = model.clone();
            }
            if let Some(effort) = retune.effort {
                meta.effort = effort.label();
            }
            // Mark it running now so a second message cannot slip in before the worker picks this one up.
            meta.status = SessionStatus::Running;
            meta.updated = now_ms();
        }
        live.save_meta();
        let typed = text.trim().to_string();
        let input = match command {
            Some(goal::Command::Set(condition)) => {
                retune.goal = Some(condition.clone());
                Input::Message { text: goal::task(&condition), shown: Some(typed), skills: added, retune, files }
            }
            Some(command) => Input::Goal { shown: typed, command, retune },
            None => Input::Message { text: text.to_string(), shown: None, skills: added, retune, files },
        };
        live.input.send(input).map_err(|_| "the session has ended".to_string())
    }

    /// Stops the run in progress. A session that is only stored has none in this process: that
    /// is not an error, unless another process is running it, which only that one can stop.
    pub fn stop(&self, id: &str) -> Result<(), String> {
        match self.live(id) {
            Ok(live) => live.cancel.lock().unwrap().cancel(),
            Err(not_live) => match self.stored(id) {
                Some(meta) if meta.why_closed.as_deref() == Some("busy") => return Err(format!("session {id} is running in another process: stop it there")),
                Some(_) => {}
                None => return Err(not_live),
            },
        }
        Ok(())
    }

    /// Ends a session for good: stops its current run, kills its background jobs and frees its
    /// memory. The log stays on disk and can still be replayed.
    pub fn close(&self, id: &str) -> Result<(), String> {
        let live = self.inner.sessions.lock().unwrap().remove(id).ok_or_else(|| format!("session {id} is not running in this process"))?;
        live.shut();
        Ok(())
    }

    /// Closes every session this process runs, as the app does when it quits. What each has
    /// stored by then is enough to continue it: a run in progress is ended and its snapshot written.
    pub fn close_all(&self) -> usize {
        let all: Vec<Arc<Live>> = self.inner.sessions.lock().unwrap().drain().map(|(_, live)| live).collect();
        for live in &all {
            live.shut();
        }
        all.len()
    }

    pub fn approve(&self, id: &str, approval: u64, allow: bool) -> Result<(), String> {
        let Ok(live) = self.live(id) else {
            // What was waiting for an answer died with the process that asked.
            return Err(if self.stored(id).is_some() { format!("session {id} is not running: no approval is waiting") } else { format!("no session {id}") });
        };
        if live.approvals.resolve(approval, allow) { Ok(()) } else { Err("that approval request is no longer pending".into()) }
    }

    /// Everything so far plus a live feed, with no gap between the two. A session that is only
    /// stored has no feed: continue it with `send_with` or `resume` first, or read its log with `events`.
    pub fn subscribe(&self, id: &str) -> Result<(Vec<Event>, broadcast::Receiver<Event>), String> {
        let live = match self.live(id) {
            Ok(live) => live,
            Err(not_live) if self.stored(id).is_none() => return Err(not_live),
            Err(_) => return Err(format!("session {id} is stored, not running: send it a message to continue it, or read its log with `events`")),
        };
        let backlog = live.events.lock().unwrap();
        Ok((backlog.clone(), live.tx.subscribe()))
    }

    /// Stored events of any session, running or past.
    pub fn events(&self, id: &str) -> Result<Vec<Event>, String> {
        if let Ok(live) = self.live(id) {
            return Ok(live.events.lock().unwrap().clone());
        }
        if id.contains(['/', '\\', '.']) {
            return Err("invalid session id".into());
        }
        let path = self.inner.data_dir.join("sessions").join(id).join("events.jsonl");
        let text = std::fs::read_to_string(&path).map_err(|_| format!("no session {id}"))?;
        Ok(text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
    }

    /// A session's meta: its live one, or what a stored session's is as `list` shows it.
    pub fn meta(&self, id: &str) -> Result<SessionMeta, String> {
        match self.live(id) {
            Ok(live) => Ok(live.meta.lock().unwrap().clone()),
            Err(not_live) => self.stored(id).ok_or(not_live),
        }
    }

    /// Whether this process is running the session: the one place a stored session differs from
    /// a live one in what can be done with it.
    pub fn is_live(&self, id: &str) -> bool {
        self.inner.sessions.lock().unwrap().contains_key(id)
    }

    /// All sessions, newest first. Ones from earlier runs of the app are listed as `Resumable` where
    /// they can be continued and as `Closed`, with `why_closed`, where they cannot.
    pub fn list(&self) -> Vec<SessionMeta> {
        let live: HashMap<String, SessionMeta> = self.inner.sessions.lock().unwrap().iter().map(|(id, l)| (id.clone(), l.meta.lock().unwrap().clone())).collect();
        let mut all: Vec<SessionMeta> = live.values().cloned().collect();
        for entry in std::fs::read_dir(self.inner.data_dir.join("sessions")).into_iter().flatten().flatten() {
            let Ok(name) = entry.file_name().into_string() else { continue };
            if live.contains_key(&name) {
                continue;
            }
            if let Some(meta) = std::fs::read_to_string(entry.path().join("meta.json")).ok().and_then(|t| serde_json::from_str::<SessionMeta>(&t).ok()) {
                all.push(self.as_stored(meta, &entry.path()));
            }
        }
        all.sort_by(|a, b| b.created.cmp(&a.created).then(b.id.cmp(&a.id)));
        all
    }

    /// Full text of an output the model was shown only in part (`out:N`).
    pub fn read_output(&self, id: &str, handle: &str) -> Result<String, String> {
        let n: u32 = handle.trim().trim_start_matches("out:").parse().map_err(|_| format!("{handle:?} is not an output handle"))?;
        if id.contains(['/', '\\', '.']) {
            return Err("invalid session id".into());
        }
        let path = self.inner.data_dir.join("sessions").join(id).join("out").join(format!("{n}.log"));
        // Only its start is shown, so only that is read: a command can have printed gigabytes.
        let mut bytes = Vec::new();
        std::fs::File::open(&path).and_then(|file| file.take(2 * 1024 * 1024).read_to_end(&mut bytes)).map_err(|_| format!("no saved output {handle} in session {id}"))?;
        Ok(crate::text::clean(&bytes))
    }

    /// Sub-directories of `path`, for the in-app folder picker.
    pub fn list_dir(&self, path: Option<&str>) -> Result<DirListing, String> {
        let dir = match path.map(str::trim).filter(|p| !p.is_empty()) {
            Some("~") => home_dir(),
            Some(p) => p.strip_prefix("~/").map(|rest| home_dir().join(rest)).unwrap_or_else(|| PathBuf::from(p)),
            None => home_dir(),
        };
        let dir = crate::util::canonical(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        if !dir.is_dir() {
            return Err(format!("{} is not a directory", dir.display()));
        }
        let mut dirs: Vec<DirEntry> = std::fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                (!name.starts_with('.')).then(|| DirEntry { path: e.path().to_string_lossy().into_owned(), name })
            })
            .collect();
        dirs.sort_by_key(|a| a.name.to_lowercase());
        Ok(DirListing { is_git: dir.join(".git").exists(), parent: dir.parent().map(|p| p.to_string_lossy().into_owned()), path: dir.to_string_lossy().into_owned(), dirs })
    }
}

/// Continuing sessions that are stored.
impl Harness {
    fn session_dir(&self, id: &str) -> Result<PathBuf, String> {
        if id.is_empty() || id.contains(['/', '\\', '.']) {
            return Err("invalid session id".into());
        }
        Ok(self.inner.data_dir.join("sessions").join(id))
    }

    /// A stored session as `list` shows it, or none if there is no such session.
    fn stored(&self, id: &str) -> Option<SessionMeta> {
        let dir = self.session_dir(id).ok()?;
        let meta = std::fs::read_to_string(dir.join("meta.json")).ok().and_then(|t| serde_json::from_str::<SessionMeta>(&t).ok())?;
        Some(self.as_stored(meta, &dir))
    }

    /// `meta` as a session that is not running in this process stands: continuable or not, and why not.
    fn as_stored(&self, mut meta: SessionMeta, dir: &Path) -> SessionMeta {
        // A session a stand-in played has no model to go on with, unless the one who runs this
        // harness brings a stand-in of their own.
        let why = if meta.simulated && !self.inner.stand_ins.load(Ordering::Relaxed) {
            Some("simulated")
        } else if !dir.join(resume::SNAPSHOT).is_file() {
            Some("older")
        } else if SessionLock::held(dir) {
            Some("busy")
        } else if !Path::new(&meta.cwd).is_dir() {
            Some("gone")
        } else {
            None
        };
        meta.status = if why.is_some() { SessionStatus::Closed } else { SessionStatus::Resumable };
        meta.why_closed = why.map(String::from);
        meta
    }

    /// Brings a stored session back to life without sending it anything: its conversation and the
    /// rest of what it kept are restored, the log is reopened for appending and it waits for a
    /// message, idle. A session already running in this process is returned as it is. It is
    /// resumed with the model its meta names, through the current configuration: an error says
    /// so where the key is gone.
    pub fn resume(&self, id: &str) -> Result<SessionMeta, String> {
        self.revive(id, None, &Changes::default()).map(|live| live.meta.lock().unwrap().clone())
    }

    /// Lists the sessions a stand-in played as continuable, for a front end that resumes them with
    /// `resume_scripted` (tests, the desktop app's headless hooks). Without it they are read-only.
    pub fn resume_stand_ins(&self, on: bool) {
        self.inner.stand_ins.store(on, Ordering::Relaxed);
    }

    /// `resume` for a session that a stand-in played, with a stand-in to play it from here on. For
    /// tests, and for the desktop app's headless hooks: a real session never needs one.
    pub fn resume_scripted(&self, id: &str, provider: MockProvider) -> Result<SessionMeta, String> {
        self.revive(id, Some(provider), &Changes::default()).map(|live| live.meta.lock().unwrap().clone())
    }

    /// The session, live: as it is if this process runs it, else restored from what it stored.
    /// `changes` are the message's own, which may name the provider to use in place of the
    /// stored one: one whose key is gone does not stand in the way of that.
    fn revive(&self, id: &str, stand_in: Option<MockProvider>, changes: &Changes) -> Result<Arc<Live>, String> {
        let dir = self.session_dir(id)?;
        // Held to the end: two messages at once must not both bring the session back.
        let mut sessions = self.inner.sessions.lock().unwrap();
        if let Some(live) = sessions.get(id) {
            return Ok(live.clone());
        }
        let cannot = |why: String| format!("session {id} cannot be continued: {why}");
        let mut meta = std::fs::read_to_string(dir.join("meta.json")).ok().and_then(|t| serde_json::from_str::<SessionMeta>(&t).ok()).ok_or_else(|| format!("no session {id}"))?;
        if meta.simulated != stand_in.is_some() {
            return Err(cannot(if meta.simulated { "it was played by a stand-in, which only a test can bring back".into() } else { "it was not played by a stand-in".into() }));
        }
        let snapshot = resume::read(&dir).map_err(cannot)?;
        if !Path::new(&meta.cwd).is_dir() {
            return Err(cannot(format!("its folder {} is gone", meta.cwd)));
        }
        let lock = SessionLock::acquire(&dir).map_err(cannot)?;
        let provider = match stand_in {
            Some(stand_in) => Provider::Mock(stand_in),
            None => {
                let config = self.inner.config.lock().unwrap();
                match config.resolve(Some(&meta.provider), Some(&meta.model)) {
                    Ok(resolved) => self.client(&resolved)?,
                    // The message names another model: that one is used, and the stored one need not resolve.
                    Err(_) if changes.provider.is_some() || changes.model.is_some() => {
                        let resolved = config.resolve(Some(changes.provider.as_deref().unwrap_or(&meta.provider)), changes.model.as_deref())?;
                        self.client(&resolved)?
                    }
                    Err(why) => return Err(cannot(format!("its model {} through {} cannot be reached now: {why}", meta.model, meta.provider))),
                }
            }
        };

        let ws = Workspace::new(Path::new(&meta.cwd)).map_err(|e| cannot(format!("cannot use {:?} as a workspace: {e}", meta.cwd)))?;
        let (features, mode) = features_of(&StartRequest { mode: Some(snapshot.setup.mode.clone()), with: snapshot.setup.with.clone(), without: snapshot.setup.without.clone(), ..Default::default() }).map_err(cannot)?;
        let _ = crate::util::private_dir(&self.inner.data_dir.join("sessions"));
        let sess = Session::new(ws, dir.clone(), features, prices_for(&meta.model)).map_err(|e| cannot(format!("could not open the session directory: {e}")))?;
        let mut cfg = AgentConfig::new(&meta.model);
        cfg.features = features;
        let setup = Setup { mode: mode.as_str().to_string(), with: snapshot.setup.with.clone(), without: snapshot.setup.without.clone() };
        let (cut_off, stats) = (snapshot.in_run, snapshot.stats.clone());

        // The log as it was: what a subscriber is shown first, and where the numbering goes on from.
        let log_path = dir.join("events.jsonl");
        let text = std::fs::read_to_string(&log_path).unwrap_or_default();
        let backlog: Vec<Event> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        let last = backlog.iter().map(|e| e.seq).max().unwrap_or(0);
        let mut log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| cannot(format!("cannot open its log: {e}")))?;
        // A write cut short leaves a line with no end: the next event must not join it.
        if !text.is_empty() && !text.ends_with('\n') {
            let _ = writeln!(log);
        }

        meta.status = SessionStatus::Idle;
        meta.why_closed = None;
        meta.stats = stats;
        meta.updated = now_ms();
        let (tx, _) = broadcast::channel(4096);
        let (input, inbox) = mpsc::unbounded_channel::<Input>();
        let approvals = Arc::new(Approvals::default());
        let live = Arc::new(Live { dir, meta: Mutex::new(meta), events: Mutex::new(backlog), seq: AtomicU64::new(last), tx, log: Mutex::new(Some(log)), input, cancel: Mutex::new(CancellationToken::new()), closed: CancellationToken::new(), approvals: approvals.clone(), lock: Mutex::new(Some(lock)) });
        let goal = live.meta.lock().unwrap().goal.clone();
        let sink: Arc<dyn Sink> = live.clone();
        let (mut agent, answered) = Agent::restore(provider, sess, cfg, sink, approvals, snapshot, goal);
        agent.keep_snapshots(live.dir.join(resume::SNAPSHOT), setup);
        live.save_meta();
        live.emit(EventKind::Resumed { cut_off, answered });
        spawn_worker(live.clone(), agent, inbox);
        sessions.insert(id.to_string(), live.clone());
        Ok(live)
    }
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn harness(name: &str) -> Harness {
        Harness::at(std::env::temp_dir().join(format!("sage-harness-{name}-{}", rand_hex(4))))
    }

    async fn until_idle(h: &Harness, id: &str) -> SessionMeta {
        for _ in 0..400 {
            let m = h.meta(id).unwrap();
            if m.status == SessionStatus::Idle {
                return m;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("session did not become idle");
    }

    #[tokio::test]
    async fn demo_session_streams_persists_and_can_be_listed_and_replayed() {
        let h = harness("demo");
        let meta = h.demo().unwrap();
        assert!(meta.simulated);
        let (backlog, mut rx) = h.subscribe(&meta.id).unwrap();
        assert!(matches!(backlog.first().map(|e| &e.kind), Some(EventKind::SessionStart { simulated: true, .. })));

        let mut live_seqs = Vec::new();
        let mut saw_delta = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(30), rx.recv()).await.expect("events keep coming").unwrap();
            saw_delta |= ev.kind.transient();
            live_seqs.push(ev.seq);
            if matches!(ev.kind, EventKind::Finish { .. }) {
                break;
            }
        }
        assert!(saw_delta, "reasoning is streamed");
        assert!(live_seqs.windows(2).all(|w| w[1] == w[0] + 1), "no gaps in the live feed");

        let done = until_idle(&h, &meta.id).await;
        assert_eq!((done.outcome, done.verified), (Some(FinishStatus::Done), Some(true)));
        assert_eq!((done.stats.turns, done.stats.surprises), (3, 1));

        // The log on disk holds every stored event, in order, and no streaming deltas.
        let stored = h.events(&meta.id).unwrap();
        assert!(stored.iter().all(|e| !e.kind.transient()));
        assert!(stored.windows(2).all(|w| w[1].seq > w[0].seq));
        let on_disk = std::fs::read_to_string(h.inner.data_dir.join("sessions").join(&meta.id).join("events.jsonl")).unwrap();
        assert_eq!(on_disk.lines().count(), stored.len());

        // A fresh process sees it as a closed session and can replay it.
        let later = Harness::at(h.inner.data_dir.clone());
        let listed = later.list();
        assert_eq!((listed.len(), listed[0].status, listed[0].verified), (1, SessionStatus::Closed, Some(true)));
        assert_eq!(later.events(&meta.id).unwrap().len(), stored.len());
        assert!(later.send_with_skills(&meta.id, "more", &[]).is_err(), "closed sessions cannot be continued");
        assert!(later.read_output(&meta.id, "out:1").unwrap().contains("FAIL: expected 60, got 16"));
        assert!(later.events("../../etc").is_err());
    }

    #[tokio::test]
    async fn keys_defaults_and_start_validation() {
        let h = harness("keys");
        assert!(h.set_key("nope", "k").is_err());
        h.set_key("openrouter", "sk-test").unwrap();
        let state = h.state();
        assert!(state.providers.iter().find(|p| p.id == "openrouter").unwrap().has_key);
        assert!(!serde_json::to_string(&state).unwrap().contains("sk-test"), "keys never leave through the state");
        assert!(h.set_defaults(None, None, Some("turbo".into())).is_err());
        h.set_defaults(Some("openrouter".into()), Some("z-ai/glm-5.3".into()), Some("max".into())).unwrap();
        let state = Harness::at(h.inner.data_dir.clone()).state();
        assert_eq!((state.provider.as_deref(), state.model.as_deref(), state.effort.as_str()), (Some("openrouter"), Some("z-ai/glm-5.3"), "max"));
        // Another provider with no model named: the first provider's model is not carried over to it.
        h.set_defaults(Some("deepseek".into()), None, None).unwrap();
        assert_eq!(h.inner.config.lock().unwrap().model, None);
        h.set_defaults(Some("openrouter".into()), Some("z-ai/glm-5.3".into()), None).unwrap();

        let bad_dir = h.start(StartRequest { cwd: "/definitely/not/here".into(), task: "x".into(), provider: Some("openrouter".into()), ..Default::default() });
        assert!(bad_dir.unwrap_err().contains("cannot use"));
        assert!(h.start(StartRequest { cwd: "/tmp".into(), task: "  ".into(), ..Default::default() }).unwrap_err().contains("empty"));
        h.set_key("openrouter", "").unwrap();
        assert!(!h.state().providers.iter().find(|p| p.id == "openrouter").unwrap().has_key || std::env::var("OPENROUTER_API_KEY").is_ok());

        // An endpoint of the user's own: listed after the built-in ones, kept, and taken away with its key.
        assert!(h.add_provider("Ollama", "ftp://localhost", &["qwen3".into()], "").is_err());
        let id = h.add_provider("Ollama", "http://localhost:11434/v1", &["qwen3".into()], " sk-own ").unwrap();
        h.set_key(&id, "sk-other").unwrap();
        h.set_defaults(Some(id.clone()), Some("qwen3".into()), None).unwrap();
        let state = Harness::at(h.inner.data_dir.clone()).state();
        let own = state.providers.last().unwrap();
        assert_eq!((own.id.as_str(), own.name.as_str(), own.custom, own.has_key, own.models.as_slice()), ("custom-ollama", "Ollama", true, true, &["qwen3".to_string()][..]));
        assert_eq!((state.provider.as_deref(), state.model.as_deref()), (Some("custom-ollama"), Some("qwen3")));
        assert!(!serde_json::to_string(&state).unwrap().contains("sk-other"));
        assert!(h.remove_provider("openrouter").is_err());
        h.remove_provider(&id).unwrap();
        let state = Harness::at(h.inner.data_dir.clone()).state();
        assert!(state.providers.iter().all(|p| !p.custom) && state.provider.as_deref() != Some("custom-ollama"));
        assert!(h.set_key(&id, "k").is_err());
    }

    #[tokio::test]
    async fn scripted_sessions_can_switch_mechanisms_off_and_be_closed() {
        use crate::provider::mock::MockTurn;
        use serde_json::json;
        let h = harness("scripted");
        let ws = h.inner.data_dir.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let cwd = ws.to_string_lossy().into_owned();
        let turns = vec![MockTurn::calls("", vec![("write", json!({"path": "a.txt", "content": "x"})), ("shell", json!({"cmd": "test -f a.txt", "expect_exit": "0"})), ("finish", json!({"summary": "wrote a.txt"}))])];
        let req = StartRequest { cwd: cwd.clone(), task: "write a.txt".into(), without: vec!["auto-watch".into(), "survey".into()], ..Default::default() };
        let meta = h.start_scripted(req, MockProvider::scripted(turns)).unwrap();
        assert_eq!((meta.mode.as_str(), meta.simulated), ("ecosage without auto-watch, survey", true));
        let done = until_idle(&h, &meta.id).await;
        assert_eq!((done.outcome, done.verified), (Some(FinishStatus::Done), Some(true)));

        // Closing frees the session; its log can still be replayed, and it is listed as stored: a
        // stand-in's cannot be continued.
        h.close(&meta.id).unwrap();
        assert!(h.close(&meta.id).is_err() && !h.is_live(&meta.id));
        let stored = h.meta(&meta.id).unwrap();
        assert_eq!((stored.status, stored.why_closed.as_deref()), (SessionStatus::Closed, Some("simulated")));
        assert!(h.events(&meta.id).unwrap().iter().any(|e| matches!(e.kind, EventKind::Finish { .. })));

        let bad = StartRequest { cwd, task: "x".into(), without: vec!["telepathy".into()], ..Default::default() };
        assert!(h.start_scripted(bad, MockProvider::scripted(vec![])).unwrap_err().contains("unknown mechanism"));

        // A harness that shares the keys but keeps its sessions elsewhere.
        h.set_key("openrouter", "sk-test").unwrap();
        let other = h.with_data_dir(h.inner.data_dir.join("elsewhere"));
        assert!(other.state().providers.iter().any(|p| p.id == "openrouter" && p.has_key));
        assert!(other.list().is_empty());

        // Auto-retry is kept with the defaults, and is not handed on to a benchmark's harness.
        assert!(!h.state().auto_retry);
        h.set_auto_retry(true).unwrap();
        assert!(h.state().auto_retry && Harness::at(h.inner.data_dir.clone()).state().auto_retry);
        assert!(!h.with_data_dir(h.inner.data_dir.join("bench")).state().auto_retry);
        h.set_auto_retry(false).unwrap();
        assert!(!h.state().auto_retry && !Harness::at(h.inner.data_dir.clone()).state().auto_retry);
    }

    #[tokio::test]
    async fn the_system_template_and_the_users_instructions_can_be_edited_and_reset() {
        let h = harness("instructions");
        let fresh = h.instructions();
        assert!(!fresh.system_custom && fresh.system == fresh.system_default && fresh.global.is_empty());
        assert!(fresh.system_default.starts_with("You are Sage") && fresh.system_default.contains("{watch}"));

        h.set_system_prompt(Some("You are terse. {watch}")).unwrap();
        h.set_global_instructions("Use tabs.\n").unwrap();
        let edited = Harness::at(h.inner.data_dir.clone()).instructions();
        assert_eq!((edited.system.as_str(), edited.system_custom, edited.global.as_str()), ("You are terse. {watch}", true, "Use tabs.\n"));
        assert_eq!(edited.system_default, fresh.system_default);

        // The built-in text, nothing and `None` all go back to the built-in template.
        for reset in [Some(fresh.system_default.as_str()), Some(" \n"), None] {
            h.set_system_prompt(Some("Mine.")).unwrap();
            assert!(h.instructions().system_custom);
            h.set_system_prompt(reset).unwrap();
            assert!(!h.instructions().system_custom && !h.inner.data_dir.join("system.md").exists());
        }
        h.set_global_instructions("").unwrap();
        assert!(h.instructions().global.is_empty() && !h.inner.data_dir.join("AGENTS.md").exists());
    }

    #[tokio::test]
    async fn a_session_gets_its_system_prompt_once_and_later_skills_with_the_message() {
        use crate::provider::mock::{MockTurn, TurnFn};
        use crate::provider::{Message, Role};
        let h = harness("skills");
        let ws = h.inner.data_dir.join("ws");
        std::fs::create_dir_all(ws.join(".sage/skills")).unwrap();
        std::fs::write(ws.join("AGENTS.md"), "Run make test.\n").unwrap();
        std::fs::write(ws.join(".sage/skills/house.md"), "---\ntitle: House rules\n---\nNo globals.\n").unwrap();
        h.set_global_instructions("Use tabs.").unwrap();
        let cwd = ws.to_string_lossy().into_owned();
        let req = || StartRequest { cwd: cwd.clone(), task: "hello".into(), interactive: true, skills: vec!["security".into(), "house".into()], ..Default::default() };
        let skill = |name: &str| h.skills(Some(&cwd)).into_iter().find(|s| s.name == name).unwrap();

        // Template, the user's instructions, the project's, then the skills in the order asked for.
        let built_in = prompt::system_prompt(&Features::ecosage());
        let expected = h.system_prompt(&req()).unwrap();
        let (user, project) = (prompt::USER_NOTE, prompt::project_note("AGENTS.md"));
        assert_eq!(expected, format!("{built_in}\n\n# User instructions\n\n{user}\n\nUse tabs.\n\n# Project instructions (AGENTS.md)\n\n{project}\n\nRun make test.\n\n# Skill: {}\n\n{}\n\n# Skill: House rules\n\nNo globals.", skill("security").title, skill("security").body));

        // A template for one run wins over the user's, which wins over the built-in one.
        let bare = StartRequest { skills: vec![], project_instructions: Some(false), ..req() };
        h.set_system_prompt(Some("Mine. {finish}")).unwrap();
        assert_eq!(h.system_prompt(&bare).unwrap(), format!("Mine. Watched tests are re-run first and it is refused if they fail.\n\n# User instructions\n\n{user}\n\nUse tabs."));
        assert_eq!(h.system_prompt(&StartRequest { system_prompt: Some("Be terse.\n".into()), ..bare.clone() }).unwrap(), format!("Be terse.\n\n# User instructions\n\n{user}\n\nUse tabs."));
        h.set_system_prompt(None).unwrap();
        assert_eq!(h.system_prompt(&bare).unwrap(), format!("{built_in}\n\n# User instructions\n\n{user}\n\nUse tabs."));

        // The baseline is the classic prompt and nothing else.
        assert_eq!(h.system_prompt(&StartRequest { mode: Some("classic".into()), system_prompt: Some("Be terse.".into()), ..req() }).unwrap(), prompt::SYSTEM_CLASSIC);
        let no_scripts = h.system_prompt(&StartRequest { without: vec!["scripts".into()], ..bare.clone() }).unwrap();
        assert_eq!(no_scripts, format!("{}\n\n# User instructions\n\n{user}\n\nUse tabs.", prompt::SYSTEM_CLASSIC));
        assert!(h.system_prompt(&StartRequest { with: vec!["telepathy".into()], ..bare.clone() }).unwrap_err().contains("unknown mechanism"));

        let unknown = StartRequest { skills: vec!["ui".into(), "telepathy".into()], ..req() };
        assert!(h.system_prompt(&unknown).unwrap_err().contains("unknown skill \"telepathy\""));
        assert!(h.start_scripted(unknown, MockProvider::scripted(vec![])).unwrap_err().contains("unknown skill \"telepathy\""));
        assert!(h.list().is_empty(), "nothing was started");

        let seen: Arc<Mutex<Vec<Vec<Message>>>> = Arc::default();
        let turn = || {
            let seen = seen.clone();
            Box::new(move |messages: &[Message]| {
                seen.lock().unwrap().push(messages.to_vec());
                MockTurn::say("ok")
            }) as TurnFn
        };
        let meta = h.start_scripted(req(), MockProvider::new(vec![turn(), turn(), turn()])).unwrap();
        assert_eq!(meta.skills, ["security", "house"]);
        until_idle(&h, &meta.id).await;

        // What changes on disk after the start does not reach a session that is under way.
        std::fs::write(ws.join("AGENTS.md"), "Changed.").unwrap();
        h.set_global_instructions("Changed.").unwrap();
        h.send_with_skills(&meta.id, "now the UI", &["house".into(), "ui".into()]).unwrap();
        until_idle(&h, &meta.id).await;
        h.send_with_skills(&meta.id, "again", &["ui".into()]).unwrap();
        let done = until_idle(&h, &meta.id).await;
        assert_eq!(done.skills, ["security", "house", "ui"]);
        assert!(h.send_with_skills(&meta.id, "more", &["telepathy".into()]).unwrap_err().contains("unknown skill"));
        assert_eq!(h.meta(&meta.id).unwrap().status, SessionStatus::Idle);

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert!(seen.iter().all(|messages| messages[0].content == expected), "the system prompt is the same bytes in every request");
        let said: Vec<&str> = seen[2].iter().filter(|m| m.role == Role::User).map(|m| m.content.as_str()).collect();
        assert!(said[0].ends_with("<task>\nhello\n</task>") && !said[0].contains("<skill") && !said[0].contains("Run make test."), "{}", said[0]);
        // Only the skill the session did not have yet, and only once.
        assert_eq!(said[1], format!("<skill name=\"ui\">\n{}\n</skill>\n\nnow the UI", skill("ui").body));
        assert_eq!(said[2], "again");

        let events = h.events(&meta.id).unwrap();
        assert!(matches!(&events[0].kind, EventKind::SessionStart { skills, .. } if *skills == ["security", "house"]));
        let with_message: Vec<(String, Vec<String>)> = events.iter().filter_map(|e| if let EventKind::User { text, skills, .. } = &e.kind { Some((text.clone(), skills.clone())) } else { None }).collect();
        assert_eq!(with_message, [("hello".to_string(), vec![]), ("now the UI".to_string(), vec!["ui".to_string()]), ("again".to_string(), vec![])]);
        assert_eq!(Harness::at(h.inner.data_dir.clone()).list()[0].skills, ["security", "house", "ui"]);
    }

    #[tokio::test]
    async fn a_session_runs_in_one_of_three_modes() {
        use crate::provider::mock::MockTurn;
        let h = harness("modes");
        let ws = h.inner.data_dir.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let req = |mode: Option<&str>| StartRequest { cwd: ws.to_string_lossy().into_owned(), task: "hello".into(), mode: mode.map(str::to_string), interactive: true, ..Default::default() };
        // What each is called in the session's record. No mode chooses the effort for the user.
        for (asked, mode, effort) in [(None, "ecosage", "ratchet-low"), (Some("ecosage"), "ecosage", "ratchet-low"), (Some("default"), "default", "ratchet-low"), (Some("classic"), "default", "ratchet-low"), (Some("Prosage"), "prosage", "ratchet-low")] {
            let meta = h.start_scripted(req(asked), MockProvider::scripted(vec![MockTurn::say("ok")])).unwrap();
            assert_eq!((meta.mode.as_str(), meta.effort.as_str()), (mode, effort), "{asked:?}");
            until_idle(&h, &meta.id).await;
        }
        assert_eq!(h.start_scripted(req(Some("turbo")), MockProvider::scripted(vec![])).unwrap_err(), "unknown mode \"turbo\"; use ecosage, default, prosage");
        // An effort that was chosen stands, and Prosage can be run without a part of itself.
        let meta = h.start_scripted(StartRequest { effort: Some("low".into()), without: vec!["examiner".into()], ..req(Some("prosage")) }, MockProvider::scripted(vec![MockTurn::say("ok")])).unwrap();
        assert_eq!((meta.mode.as_str(), meta.effort.as_str()), ("prosage without examiner", "low"));
        // What is stored for a restart names the mode, so the session comes back as what it was.
        until_idle(&h, &meta.id).await;
        let setup = resume::read(&session_dir(&h, &meta.id)).unwrap().setup;
        assert_eq!((setup.mode.as_str(), setup.without.as_slice()), ("prosage", &["examiner".to_string()][..]));
        let back = features_of(&StartRequest { mode: Some(setup.mode), without: setup.without, ..Default::default() }).unwrap();
        assert_eq!(back, (Features::prosage().without("examiner").unwrap(), Mode::Prosage));

        // Prosage is Ecosage and what Ecosage leaves out to stay cheap; the conventional loop has none of either.
        let (pro, plain) = (Mode::Prosage.features(), Mode::Plain.features());
        assert!(pro.long_thought && pro.unhurried && pro.examiner && pro.panel && pro.referee && pro.routing && pro.scripts && pro.gate);
        assert!(!plain.scripts && !plain.examiner && !plain.referee && !plain.routing && Mode::Ecosage.features() == Features::ecosage());
        assert_eq!(Features::OPTIONAL.iter().fold(Features::ecosage(), |f, name| f.with(name).unwrap()), pro);
    }

    #[tokio::test]
    async fn the_classic_baseline_takes_no_skills() {
        use crate::provider::mock::MockTurn;
        let h = harness("classic-skills");
        let ws = h.inner.data_dir.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let req = StartRequest { cwd: ws.to_string_lossy().into_owned(), task: "hello".into(), mode: Some("classic".into()), interactive: true, skills: vec!["ui".into()], ..Default::default() };
        let meta = h.start_scripted(req, MockProvider::scripted(vec![MockTurn::say("ok"), MockTurn::say("ok")])).unwrap();
        assert!(meta.skills.is_empty());
        until_idle(&h, &meta.id).await;
        h.send_with_skills(&meta.id, "more", &["security".into()]).unwrap();
        assert!(until_idle(&h, &meta.id).await.skills.is_empty());
        assert!(h.events(&meta.id).unwrap().iter().all(|e| !matches!(&e.kind, EventKind::User { skills, .. } if !skills.is_empty())));
    }

    #[tokio::test]
    async fn a_message_can_change_the_effort_and_the_autonomy_of_its_session() {
        use crate::provider::mock::MockTurn;
        let h = harness("changes");
        let ws = h.inner.data_dir.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let req = StartRequest { cwd: ws.to_string_lossy().into_owned(), task: "hello".into(), interactive: true, ..Default::default() };
        let meta = h.start_scripted(req, MockProvider::scripted(vec![MockTurn::say("ok"), MockTurn::say("ok"), MockTurn::say("ok"), MockTurn::say("ok")])).unwrap();
        assert_eq!(until_idle(&h, &meta.id).await.effort, "ratchet-low");

        // What cannot be done is refused before anything is sent.
        let change = |changes: Changes| h.send_with(&meta.id, "more", &[], &[], changes);
        assert!(change(Changes { effort: Some("turbo".into()), ..Default::default() }).unwrap_err().contains("unknown effort"));
        assert!(change(Changes { autonomy: Some("maybe".into()), ..Default::default() }).unwrap_err().contains("unknown autonomy"));
        assert!(change(Changes { model: Some("another".into()), ..Default::default() }).unwrap_err().contains("stand-in"));
        assert_eq!(h.meta(&meta.id).unwrap().status, SessionStatus::Idle);

        change(Changes { effort: Some("max".into()), autonomy: Some("ask".into()), ..Default::default() }).unwrap();
        assert_eq!(until_idle(&h, &meta.id).await.effort, "max");
        // Naming what the session has already is no change.
        change(Changes { provider: Some("scripted".into()), model: Some("scripted stand-in".into()), effort: Some("max".into()), autonomy: Some("ask".into()) }).unwrap();
        until_idle(&h, &meta.id).await;
        let events = h.events(&meta.id).unwrap();
        let settings: Vec<&EventKind> = events.iter().map(|e| &e.kind).filter(|k| matches!(k, EventKind::Settings { .. })).collect();
        assert!(matches!(settings.as_slice(), [EventKind::Settings { provider: None, model: None, effort: Some(effort), autonomy: Some(autonomy) }] if effort == "max" && autonomy == "ask"), "{settings:?}");
        // It is kept with the session, for the listing and for a later launch.
        assert_eq!(Harness::at(h.inner.data_dir.clone()).list()[0].effort, "max");

        // A file goes with a message when it is there to be read, and is named in the log.
        let note = ws.join("note.txt");
        std::fs::write(&note, "remember").unwrap();
        let note = note.to_string_lossy().into_owned();
        assert!(h.send_with(&meta.id, "more", &[], &["/no/such/file".into()], Changes::default()).unwrap_err().contains("cannot attach"));
        h.send_with(&meta.id, "more", &[], &[note.clone(), note.clone()], Changes::default()).unwrap();
        until_idle(&h, &meta.id).await;
        assert!(matches!(&h.events(&meta.id).unwrap().iter().rfind(|e| matches!(e.kind, EventKind::User { .. })).unwrap().kind, EventKind::User { files, .. } if *files == [note]));
    }

    #[test]
    fn what_was_stored_before_there_were_skills_still_loads() {
        let meta = SessionMeta { id: "s".into(), title: "t".into(), cwd: "/".into(), provider: "p".into(), model: "m".into(), mode: "ecosage".into(), effort: "high".into(), simulated: false, created: 1, updated: 2, status: SessionStatus::Idle, outcome: None, verified: None, stats: Stats::default(), skills: vec!["ui".into()], goal: None, why_closed: None };
        let mut old = serde_json::to_value(&meta).unwrap();
        assert_eq!(old.as_object_mut().unwrap().remove("skills"), Some(serde_json::json!(["ui"])));
        assert!(serde_json::from_value::<SessionMeta>(old).unwrap().skills.is_empty());

        let start = r#"{"seq":1,"ts":1,"type":"session_start","id":"s","title":"t","cwd":"/","provider":"p","model":"m","effort":"high","mode":"ecosage","autonomy":"auto","simulated":false}"#;
        assert!(matches!(serde_json::from_str::<Event>(start).unwrap().kind, EventKind::SessionStart { skills, .. } if skills.is_empty()));
        let user = r#"{"seq":2,"ts":1,"type":"user","text":"hi"}"#;
        let event = serde_json::from_str::<Event>(user).unwrap();
        assert!(matches!(&event.kind, EventKind::User { skills, .. } if skills.is_empty()));
        assert_eq!(serde_json::to_string(&event).unwrap(), user);

        let req: StartRequest = serde_json::from_str(r#"{"cwd":"/tmp","task":"x"}"#).unwrap();
        assert!(req.skills.is_empty() && req.system_prompt.is_none() && req.project_instructions.is_none());
    }

    #[tokio::test]
    async fn folder_picker_lists_visible_directories() {
        let h = harness("dirs");
        let root = h.inner.data_dir.join("tree");
        for d in ["beta", "Alpha", ".hidden", "gamma/.git"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("file.txt"), "").unwrap();
        let listing = h.list_dir(Some(root.to_str().unwrap())).unwrap();
        assert_eq!(listing.dirs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), vec!["Alpha", "beta", "gamma"]);
        assert!(listing.parent.is_some() && !listing.is_git);
        assert!(h.list_dir(Some(root.join("gamma").to_str().unwrap())).unwrap().is_git);
        assert!(h.list_dir(Some(root.join("file.txt").to_str().unwrap())).is_err());
        assert!(h.list_dir(None).is_ok());
    }

    type Seen = Arc<Mutex<Vec<Vec<crate::provider::Message>>>>;

    /// Scripted replies that keep what each request carried, so that a test can count them.
    fn recording(seen: &Seen, turns: Vec<crate::provider::mock::MockTurn>) -> MockProvider {
        use crate::provider::mock::TurnFn;
        MockProvider::new(
            turns
                .into_iter()
                .map(|turn| {
                    let seen = seen.clone();
                    Box::new(move |messages: &[crate::provider::Message]| {
                        seen.lock().unwrap().push(messages.to_vec());
                        turn.clone()
                    }) as TurnFn
                })
                .collect(),
        )
    }

    fn goals(h: &Harness, id: &str) -> Vec<(GoalState, String, String)> {
        h.events(id).unwrap().into_iter().filter_map(|e| if let EventKind::Goal { state, text, detail } = e.kind { Some((state, text, detail)) } else { None }).collect()
    }

    fn user_messages(h: &Harness, id: &str) -> Vec<String> {
        h.events(id).unwrap().into_iter().filter_map(|e| if let EventKind::User { text, .. } = e.kind { Some(text) } else { None }).collect()
    }

    #[tokio::test]
    async fn a_goal_is_set_by_the_task_or_a_message_and_stays_until_it_is_met_or_cleared() {
        use crate::provider::mock::MockTurn;
        use serde_json::json;
        let h = harness("goal");
        let ws = h.inner.data_dir.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("n.txt"), "alpha\nbeta\n").unwrap();
        let upper = |old: &str, new: &str| MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": old, "new": new})), ("shell", json!({"cmd": format!("grep -qx {new} n.txt"), "expect_exit": "0"})), ("finish", json!({"summary": format!("{new} done")}))]);
        let finish = || MockTurn::calls("", vec![("finish", json!({"summary": "done"}))]);
        let seen = Seen::default();
        let turns = vec![
            // The task is a goal: the model works, and is sent back once.
            upper("alpha", "ALPHA"),
            MockTurn::say("NOT MET\nbeta is still lower case"),
            upper("beta", "BETA"),
            MockTurn::say("MET\nboth lines are upper case"),
            // A follow-up sets another goal; its check cannot be read, so it stays.
            finish(),
            MockTurn::say("Hard to say."),
            // The next message is checked against it, and this time it holds.
            finish(),
            MockTurn::say("MET\nit holds"),
            // Met: nothing is asked after this one.
            finish(),
        ];
        let req = StartRequest { cwd: ws.to_string_lossy().into_owned(), task: "/goal both lines of n.txt are upper case".into(), ..Default::default() };
        let meta = h.start_scripted(req, recording(&seen, turns)).unwrap();
        assert_eq!(meta.goal, Some(Goal { text: "both lines of n.txt are upper case".into(), met: false }));
        assert_eq!(meta.title, "both lines of n.txt are upper case");
        let done = until_idle(&h, &meta.id).await;
        assert_eq!((done.outcome, done.goal.clone()), (Some(FinishStatus::Done), Some(Goal { text: "both lines of n.txt are upper case".into(), met: true })));
        assert_eq!(seen.lock().unwrap().len(), 4, "two replies of the model and two checks");
        // The model is sent the goal as a task; the log shows what the user typed.
        assert!(seen.lock().unwrap()[0].last().unwrap().content.ends_with("<task>\nWork until this holds: both lines of n.txt are upper case\n</task>"));
        assert_eq!(user_messages(&h, &meta.id), ["/goal both lines of n.txt are upper case"]);
        let states: Vec<GoalState> = goals(&h, &meta.id).iter().map(|g| g.0).collect();
        assert_eq!(states, [GoalState::Set, GoalState::NotMet, GoalState::Met]);
        // It was met in the stored meta as well, not only in memory.
        assert_eq!(Harness::at(h.inner.data_dir.clone()).list()[0].goal, done.goal);

        // `/goal` says what it is and `/goal clear` drops it: neither asks the model anything.
        h.send_with(&meta.id, "/goal", &[], &[], Changes::default()).unwrap();
        until_idle(&h, &meta.id).await;
        h.send_with(&meta.id, " /goal clear ", &[], &[], Changes::default()).unwrap();
        let cleared = until_idle(&h, &meta.id).await;
        h.send_with(&meta.id, "/goal", &[], &[], Changes::default()).unwrap();
        until_idle(&h, &meta.id).await;
        assert_eq!(seen.lock().unwrap().len(), 4);
        assert_eq!((cleared.goal, cleared.outcome), (None, Some(FinishStatus::Done)));
        let said: Vec<(GoalState, String, String)> = goals(&h, &meta.id).split_off(3);
        assert_eq!(said, [(GoalState::Shown, "both lines of n.txt are upper case".into(), "met".into()), (GoalState::Cleared, "both lines of n.txt are upper case".into(), String::new()), (GoalState::Shown, String::new(), String::new())]);
        assert_eq!(user_messages(&h, &meta.id)[1..], ["/goal", "/goal clear", "/goal"]);
        assert_eq!(Harness::at(h.inner.data_dir.clone()).list()[0].goal, None);

        // A message sets a goal: the model is sent it as a task, and the goal stays while it is unmet.
        h.send_with(&meta.id, "/goal  n.txt still starts with ALPHA", &[], &[], Changes::default()).unwrap();
        let unchecked = until_idle(&h, &meta.id).await;
        assert_eq!(unchecked.goal, Some(Goal { text: "n.txt still starts with ALPHA".into(), met: false }));
        assert_eq!(seen.lock().unwrap().len(), 6);
        assert_eq!(seen.lock().unwrap()[4].last().unwrap().content, "Work until this holds: n.txt still starts with ALPHA");
        assert_eq!(user_messages(&h, &meta.id).last().unwrap(), "/goal  n.txt still starts with ALPHA");
        let last = goals(&h, &meta.id).pop().unwrap();
        assert_eq!(last.0, GoalState::Unchecked);
        // An ordinary message in between is still held to it, and ends it once the check agrees.
        assert!(matches!(h.events(&meta.id).unwrap().last().map(|e| &e.kind), Some(EventKind::Finish { evidence, .. }) if evidence.contains("goal could not be checked")));
        h.send_with(&meta.id, "Anything to add?", &[], &[], Changes::default()).unwrap();
        let met = until_idle(&h, &meta.id).await;
        assert_eq!(met.goal, Some(Goal { text: "n.txt still starts with ALPHA".into(), met: true }));
        assert_eq!(seen.lock().unwrap().len(), 8);
        // A goal that is met no longer gates the messages after it.
        h.send_with(&meta.id, "And now?", &[], &[], Changes::default()).unwrap();
        until_idle(&h, &meta.id).await;
        assert_eq!(seen.lock().unwrap().len(), 9);
        assert_eq!(Harness::at(h.inner.data_dir.clone()).list()[0].goal, met.goal);
    }

    #[tokio::test]
    async fn a_session_starts_with_a_goal_of_its_own_or_not_at_all_with_a_goal_command_that_has_none() {
        use crate::provider::mock::MockTurn;
        let h = harness("goal-start");
        let ws = h.inner.data_dir.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let cwd = ws.to_string_lossy().into_owned();
        let req = |task: &str, goal: Option<&str>| StartRequest { cwd: cwd.clone(), task: task.into(), goal: goal.map(String::from), interactive: true, ..Default::default() };
        let start = |req: StartRequest, seen: &Seen| h.start_scripted(req, recording(seen, vec![MockTurn::say("ok"), MockTurn::say("MET")]));

        // Nothing to report or clear yet: the first message has to say what the goal is.
        for task in ["/goal", "/goal clear", "  /goal  "] {
            let err = start(req(task, None), &Seen::default()).unwrap_err();
            assert!(err.contains("no goal yet") && err.contains("/goal <condition>"), "{err}");
        }
        assert!(start(req("fix it", Some("  ")), &Seen::default()).unwrap_err().contains("goal is empty"));
        assert!(h.start(StartRequest { cwd: cwd.clone(), task: " ".into(), ..Default::default() }).unwrap_err().contains("empty"));
        assert!(h.list().iter().all(|m| m.goal.is_none()), "nothing was started with a goal");

        // A goal and no task: the goal is the task.
        let seen = Seen::default();
        let meta = start(req("", Some(" the docs name every flag ")), &seen).unwrap();
        assert_eq!(meta.goal, Some(Goal { text: "the docs name every flag".into(), met: false }));
        let done = until_idle(&h, &meta.id).await;
        assert!(seen.lock().unwrap()[0].last().unwrap().content.ends_with("<task>\nWork until this holds: the docs name every flag\n</task>"));
        assert_eq!(user_messages(&h, &meta.id), ["Work until this holds: the docs name every flag"]);
        assert!(done.goal.is_some_and(|g| g.met), "a prose answer is checked too");

        // A goal beside a task: the task stays the task, and the model is told of the goal after it.
        let seen = Seen::default();
        let meta = start(req("Document the flags.", Some("the docs name every flag")), &seen).unwrap();
        until_idle(&h, &meta.id).await;
        let first = seen.lock().unwrap()[0].last().unwrap().content.clone();
        assert!(first.ends_with("<task>\nDocument the flags.\n\nThe session goal, which an independent check judges when you finish: the docs name every flag\n</task>"), "{first}");
        assert_eq!((user_messages(&h, &meta.id), meta.title.as_str()), (vec!["Document the flags.".to_string()], "Document the flags."));
    }

    #[test]
    fn logs_and_metas_that_hold_what_was_retired_still_open() {
        let h = harness("retired");
        let id = "20260101-000000-ab";
        let dir = h.inner.data_dir.join("sessions").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        // A stats record as the engine wrote it when it still counted lessons.
        let mut stats = serde_json::to_value(Stats { turns: 7, ..Stats::default() }).unwrap();
        stats.as_object_mut().unwrap().remove("goal_checks");
        stats.as_object_mut().unwrap().insert("lessons_learned".into(), 2.into());
        let line = |seq: u64, body: serde_json::Value| {
            let mut event = serde_json::json!({"seq": seq, "ts": 1});
            event.as_object_mut().unwrap().extend(body.as_object().unwrap().clone());
            event.to_string()
        };
        let log = [
            line(1, serde_json::json!({"type": "session_start", "id": id, "title": "t", "cwd": "/", "provider": "p", "model": "m", "effort": "high", "mode": "ecosage", "autonomy": "auto", "simulated": false})),
            line(2, serde_json::json!({"type": "user", "text": "hi"})),
            line(3, serde_json::json!({"type": "lesson", "learned": true, "text": "`pytest -q` fails here → `uv run pytest -q` works"})),
            line(4, serde_json::json!({"type": "note", "turn": 1, "kind": "review", "text": "one review pass"})),
            line(5, serde_json::json!({"type": "stats", "stats": stats})),
            line(6, serde_json::json!({"type": "finish", "status": "done", "summary": "ok", "verified": true, "evidence": "e"})),
        ];
        std::fs::write(dir.join("events.jsonl"), log.join("\n") + "\n").unwrap();
        let meta = SessionMeta { id: id.into(), title: "t".into(), cwd: "/".into(), provider: "p".into(), model: "m".into(), mode: "ecosage".into(), effort: "high".into(), simulated: false, created: 1, updated: 2, status: SessionStatus::Idle, outcome: Some(FinishStatus::Done), verified: Some(true), stats: Stats { turns: 7, ..Stats::default() }, skills: vec![], goal: None, why_closed: None };
        let mut old = serde_json::to_value(&meta).unwrap();
        old.as_object_mut().unwrap().remove("goal");
        old["stats"] = stats.clone();
        std::fs::write(dir.join("meta.json"), old.to_string()).unwrap();

        // The retired event is left out; everything round it is there, the old stats included.
        let events = h.events(id).unwrap();
        let kinds: Vec<u64> = events.iter().map(|e| e.seq).collect();
        assert_eq!(kinds, [1, 2, 4, 5, 6]);
        assert!(events.iter().any(|e| matches!(&e.kind, EventKind::Stats { stats } if stats.turns == 7 && stats.goal_checks == 0)));
        let listed = h.list();
        assert_eq!((listed.len(), listed[0].status, listed[0].goal.clone(), listed[0].stats.turns), (1, SessionStatus::Closed, None, 7));
        assert_eq!(listed[0].why_closed.as_deref(), Some("older"), "stored before snapshots: read-only");
    }

    // --- continuing a session after the program that ran it has ended ---

    /// The newest session as `list` gives it, once nobody has it. A session's lock is let go of when
    /// the last copy of the file it was taken on is closed, and a command that is being started at
    /// that moment holds a copy for an instant.
    async fn listed_free(h: &Harness) -> SessionMeta {
        for _ in 0..100 {
            let meta = h.list().remove(0);
            if meta.why_closed.as_deref() != Some("busy") {
                return meta;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the session stayed busy");
    }

    fn resume_workspace(h: &Harness) -> String {
        let ws = h.inner.data_dir.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("n.txt"), "alpha\nbeta\n").unwrap();
        ws.to_string_lossy().into_owned()
    }

    fn session_dir(h: &Harness, id: &str) -> PathBuf {
        h.inner.data_dir.join("sessions").join(id)
    }

    /// A first run that edits `n.txt`, is held to a goal that its checks keep finding unmet, and ends
    /// with the goal still active. Returns the harness, the session and what its requests carried.
    async fn run_with_a_goal_left_open(name: &str) -> (Harness, String, String, Seen) {
        use crate::provider::mock::MockTurn;
        use serde_json::json;
        let h = harness(name);
        let cwd = resume_workspace(&h);
        let upper = MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "alpha", "new": "ALPHA"})), ("shell", json!({"cmd": "grep -qx ALPHA n.txt", "expect_exit": "0"})), ("finish", json!({"summary": "ALPHA done"}))]);
        let finish = || MockTurn::calls("", vec![("finish", json!({"summary": "done"}))]);
        let seen = Seen::default();
        let turns = vec![upper, MockTurn::say("NOT MET\nbeta is lower case"), finish(), MockTurn::say("NOT MET\nstill"), finish(), MockTurn::say("NOT MET\nstill")];
        let req = StartRequest { cwd: cwd.clone(), task: "/goal n.txt is all upper case".into(), without: vec!["survey".into()], ..Default::default() };
        let meta = h.start_scripted(req, recording(&seen, turns)).unwrap();
        until_idle(&h, &meta.id).await;
        (h, meta.id, cwd, seen)
    }

    #[tokio::test]
    async fn a_session_is_continued_by_another_harness_with_its_conversation_numbering_and_goal() {
        use crate::provider::mock::MockTurn;
        use serde_json::json;
        let (first, id, _, first_seen) = run_with_a_goal_left_open("resume-a").await;
        let data = first.inner.data_dir.clone();
        let ended = first.meta(&id).unwrap();
        assert_eq!((ended.outcome, ended.goal.clone()), (Some(FinishStatus::Done), Some(Goal::new("n.txt is all upper case"))));
        let log_before = first.events(&id).unwrap();
        let last_seq = log_before.last().unwrap().seq;
        let snapshot = resume::read(&session_dir(&first, &id)).unwrap();
        assert!(!snapshot.in_run && snapshot.setup.without == ["survey"] && snapshot.turn == ended.stats.turns && snapshot.stats == ended.stats);

        // The session is still run here: another process, or harness, must keep out.
        let second = Harness::at(data.clone());
        second.resume_stand_ins(true);
        assert_eq!(second.list()[0].why_closed.as_deref(), Some("busy"));
        let busy = second.resume_scripted(&id, MockProvider::scripted(vec![])).unwrap_err();
        assert!(busy.contains("only one process"), "{busy}");
        assert!(second.stop(&id).unwrap_err().contains("another process"));
        drop(first);

        // Now it is stored: listed as one that can be continued, with what it had.
        let listed = vec![listed_free(&second).await];
        assert_eq!((listed[0].status, listed[0].why_closed.clone(), listed[0].stats.turns, listed[0].goal.clone()), (SessionStatus::Resumable, None, ended.stats.turns, ended.goal.clone()));
        assert_eq!(second.meta(&id).unwrap().status, SessionStatus::Resumable);
        // Nothing is running here to stop or to approve, and no live feed to give; the log can be read.
        assert!(second.stop(&id).is_ok());
        assert!(second.approve(&id, 1, true).unwrap_err().contains("not running"));
        assert!(second.subscribe(&id).unwrap_err().contains("stored, not running"));
        assert_eq!(second.events(&id).unwrap().len(), log_before.len());
        assert!(second.subscribe("nothing-like-it").unwrap_err().contains("not running in this process"));

        let seen = Seen::default();
        second.resume_scripted(&id, recording(&seen, vec![MockTurn::calls("", vec![("finish", json!({"summary": "still upper case"}))]), MockTurn::say("MET\nboth lines are upper case"), MockTurn::say("noted")])).unwrap();
        second.send_with(&id, "and keep it that way", &[], &[], Changes::default()).unwrap();
        let after = until_idle(&second, &id).await;

        // The model is sent what it had, then the new message in front of which it is told of the restart.
        let requests = seen.lock().unwrap().clone();
        let sent = &requests[0];
        assert_eq!(sent.len(), snapshot.messages.len() + 1);
        assert_eq!(sent[..snapshot.messages.len()], snapshot.messages[..]);
        let last_request_of_the_first_run = first_seen.lock().unwrap().iter().rfind(|m| m.iter().any(|m| m.content.contains("<task>"))).unwrap().clone();
        assert_eq!(sent[..last_request_of_the_first_run.len()], last_request_of_the_first_run[..], "everything the model saw last time is a prefix of what it sees now");
        let message = sent.last().unwrap();
        assert!(message.role == crate::provider::Role::User && message.content.starts_with("[harness] This session was restored after a restart"), "{}", message.content);
        assert!(message.content.ends_with("and keep it that way") && !message.content.contains("cut off"));
        assert_eq!(sent[0].role, crate::provider::Role::System);
        // The note is said once.
        second.send_with(&id, "thanks", &[], &[], Changes::default()).unwrap();
        until_idle(&second, &id).await;
        assert_eq!(seen.lock().unwrap().last().unwrap().last().unwrap().content, "thanks");

        // The log goes on from where it stopped, in order, with the resumption marked.
        let text = std::fs::read_to_string(session_dir(&second, &id).join("events.jsonl")).unwrap();
        let log: Vec<Event> = text.lines().map(|l| serde_json::from_str(l).expect("every line is an event")).collect();
        assert_eq!(log[..log_before.len()].iter().map(|e| e.seq).collect::<Vec<_>>(), log_before.iter().map(|e| e.seq).collect::<Vec<_>>());
        assert!(log.windows(2).all(|w| w[0].seq < w[1].seq));
        assert_eq!(log[log_before.len()].seq, last_seq + 1);
        assert!(matches!(log[log_before.len()].kind, EventKind::Resumed { cut_off: false, answered: 0 }));
        assert!(matches!(&log[log_before.len() + 1].kind, EventKind::User { text, .. } if text == "and keep it that way"));
        let turns: Vec<u32> = log.iter().filter_map(|e| if let EventKind::TurnStart { turn, .. } = e.kind { Some(turn) } else { None }).collect();
        assert!(turns.windows(2).all(|w| w[0] < w[1]) && *turns.last().unwrap() > ended.stats.turns, "{turns:?}");

        // Meta and stats carry on; the goal, still active when the first run ended, was met by the second.
        assert!(after.stats.turns > ended.stats.turns && after.stats.goal_checks > ended.stats.goal_checks && after.stats.cost >= ended.stats.cost);
        assert_eq!(after.goal.as_ref().map(|g| (g.text.as_str(), g.met)), Some(("n.txt is all upper case", true)));
        assert!(goals(&second, &id).iter().any(|(state, ..)| *state == GoalState::Met));
        assert_eq!(second.list()[0].title, ended.title);
        let stored: SessionMeta = serde_json::from_str(&std::fs::read_to_string(session_dir(&second, &id).join("meta.json")).unwrap()).unwrap();
        assert!(stored.why_closed.is_none() && stored.goal.unwrap().met);
        let later = resume::read(&session_dir(&second, &id)).unwrap();
        assert!(later.turn > snapshot.turn && later.messages.len() > snapshot.messages.len());
    }

    #[tokio::test]
    async fn a_snapshot_cut_off_in_the_middle_of_a_run_is_made_valid_and_the_model_is_told() {
        use crate::provider::mock::MockTurn;
        use crate::provider::Role;
        use serde_json::json;
        let h = harness("resume-cut");
        let cwd = resume_workspace(&h);
        let turns = vec![MockTurn::calls("", vec![("shell", json!({"cmd": "echo one"})), ("shell", json!({"cmd": "echo two"}))]), MockTurn::say("both said hello")];
        let id = h.start_scripted(StartRequest { cwd, task: "say hello twice".into(), without: vec!["survey".into()], ..Default::default() }, MockProvider::scripted(turns)).unwrap().id;
        until_idle(&h, &id).await;
        let dir = session_dir(&h, &id);
        let mut snapshot = resume::read(&dir).unwrap();
        // As if the program was killed while the two calls ran: the reply is stored, their results are not.
        let at = snapshot.messages.iter().rposition(|m| m.role == Role::Assistant && !m.tool_calls.is_empty()).unwrap();
        snapshot.messages.truncate(at + 1);
        snapshot.stubs.truncate(at + 1);
        snapshot.in_run = true;
        resume::write_atomic(&dir.join(resume::SNAPSHOT), &serde_json::to_vec(&snapshot).unwrap()).unwrap();
        let calls: Vec<String> = snapshot.messages[at].tool_calls.iter().map(|c| c.id.clone()).collect();
        assert_eq!(calls.len(), 2);
        drop(h);

        let second = Harness::at(dir.parent().unwrap().parent().unwrap().to_path_buf());
        let seen = Seen::default();
        second.resume_scripted(&id, recording(&seen, vec![MockTurn::say("understood")])).unwrap();
        second.send_with(&id, "go on", &[], &[], Changes::default()).unwrap();
        until_idle(&second, &id).await;

        let sent = seen.lock().unwrap()[0].clone();
        assert_eq!(sent.len(), at + 1 + 2 + 1);
        for (n, call) in calls.iter().enumerate() {
            let result = &sent[at + 1 + n];
            assert!(result.role == Role::Tool && result.tool_call_id.as_deref() == Some(call.as_str()) && result.content.contains("interrupted by a restart"), "{result:?}");
        }
        let note = &sent.last().unwrap().content;
        assert!(note.contains("cut off by the restart: the tool calls it ended on were not finished") && note.ends_with("go on"), "{note}");
        assert!(second.events(&id).unwrap().iter().any(|e| matches!(e.kind, EventKind::Resumed { cut_off: true, answered: 2 })));
    }

    #[test]
    fn dangling_calls_are_answered_where_they_stand() {
        use crate::provider::{Message, Role, ToolCall};
        let call = |id: &str| ToolCall { id: id.into(), name: "shell".into(), arguments: "{}".into() };
        let mut context = crate::context::Context::new("sys");
        context.push(Message::user("task"));
        context.push(Message { role: Role::Assistant, content: String::new(), reasoning: None, tool_calls: vec![call("a"), call("b")], tool_call_id: None });
        context.push_tool("a", "one".into(), "[a]".into());
        context.push(Message::user("[harness] carry on"));
        context.push(Message { role: Role::Assistant, content: String::new(), reasoning: None, tool_calls: vec![call("c")], tool_call_id: None });
        assert_eq!(context.answer_dangling("cut off"), 2);
        let shape: Vec<(Role, Option<&str>)> = context.messages.iter().map(|m| (m.role, m.tool_call_id.as_deref())).collect();
        assert_eq!(shape, [(Role::System, None), (Role::User, None), (Role::Assistant, None), (Role::Tool, Some("a")), (Role::Tool, Some("b")), (Role::User, None), (Role::Assistant, None), (Role::Tool, Some("c"))]);
        assert_eq!(context.stubs().len(), context.messages.len());
        assert_eq!(context.answer_dangling("cut off"), 0, "a conversation that is whole is left as it is");
    }

    #[tokio::test]
    async fn a_session_without_a_snapshot_is_read_only_and_says_why() {
        use crate::provider::mock::MockTurn;
        let h = harness("resume-older");
        let cwd = resume_workspace(&h);
        let id = h.start_scripted(StartRequest { cwd, task: "say hi".into(), ..Default::default() }, MockProvider::scripted(vec![MockTurn::say("hi")])).unwrap().id;
        until_idle(&h, &id).await;
        let data = h.inner.data_dir.clone();
        drop(h);
        let second = Harness::at(data);
        // A stand-in's session is not continued by the app unless it is asked to.
        assert_eq!(listed_free(&second).await.why_closed.as_deref(), Some("simulated"));
        second.resume_stand_ins(true);
        assert_eq!(listed_free(&second).await.status, SessionStatus::Resumable);
        // Stored before sessions kept snapshots: the log has no system prompt and not what the model
        // was shown beyond the user's words, so a conversation rebuilt from it would not be the same one.
        std::fs::remove_file(session_dir(&second, &id).join(resume::SNAPSHOT)).unwrap();
        let listed = vec![listed_free(&second).await];
        assert_eq!((listed[0].status, listed[0].why_closed.as_deref()), (SessionStatus::Closed, Some("older")));
        let refused = second.resume_scripted(&id, MockProvider::scripted(vec![])).unwrap_err();
        assert!(refused.contains("cannot be continued") && refused.contains("no snapshot"), "{refused}");
        assert!(second.send_with(&id, "more", &[], &[], Changes::default()).is_err());
        assert!(second.send_with("no-such-session", "more", &[], &[], Changes::default()).unwrap_err().contains("no session"));
        assert!(second.send_with("no-such-session", " ", &[], &[], Changes::default()).unwrap_err().contains("empty"));
        assert!(!second.events(&id).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_real_session_comes_back_through_the_current_configuration() {
        use crate::provider::mock::MockTurn;
        let (first, id, _, _) = run_with_a_goal_left_open("resume-real").await;
        let data = first.inner.data_dir.clone();
        let dir = session_dir(&first, &id);
        drop(first);
        // As if it had been started on a real provider.
        let edit = |provider: &str| {
            let mut meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("meta.json")).unwrap()).unwrap();
            meta["simulated"] = false.into();
            meta["provider"] = provider.into();
            meta["model"] = "qwen3".into();
            std::fs::write(dir.join("meta.json"), meta.to_string()).unwrap();
        };
        let h = Harness::at(data);
        let own = h.add_provider("Local", "http://127.0.0.1:9/v1", &["qwen3".into()], "").unwrap();

        // Its provider is gone: a plain message says so; one that names another model does not need it.
        edit("a-provider-that-was-removed");
        assert_eq!(listed_free(&h).await.status, SessionStatus::Resumable);
        let refused = h.resume(&id).unwrap_err();
        assert!(refused.contains("cannot be continued") && refused.contains("a-provider-that-was-removed") && refused.contains("cannot be reached"), "{refused}");
        assert!(h.resume_scripted(&id, MockProvider::scripted(vec![MockTurn::say("x")])).unwrap_err().contains("not played by a stand-in"));
        let changes = Changes { provider: Some(own.clone()), model: Some("qwen3".into()), ..Default::default() };
        h.revive(&id, None, &changes).unwrap();
        assert_eq!((h.meta(&id).unwrap().status, h.is_live(&id)), (SessionStatus::Idle, true));
        h.close(&id).unwrap();

        // Its provider is there: the session comes back idle, with nothing sent anywhere.
        edit(&own);
        let meta = h.resume(&id).unwrap();
        assert_eq!((meta.status, meta.provider.as_str(), meta.model.as_str()), (SessionStatus::Idle, own.as_str(), "qwen3"));
        assert_eq!(h.resume(&id).unwrap().id, id, "resuming a session that is live is no error");
        assert!(h.events(&id).unwrap().iter().any(|e| matches!(e.kind, EventKind::Resumed { .. })));
        assert!(h.subscribe(&id).is_ok());
    }

    #[tokio::test]
    async fn a_log_that_was_cut_short_does_not_swallow_the_next_event() {
        use crate::provider::mock::MockTurn;
        let h = harness("resume-torn");
        let cwd = resume_workspace(&h);
        let id = h.start_scripted(StartRequest { cwd, task: "say hi".into(), ..Default::default() }, MockProvider::scripted(vec![MockTurn::say("hi"), MockTurn::say("again")])).unwrap().id;
        until_idle(&h, &id).await;
        let data = h.inner.data_dir.clone();
        let log = session_dir(&h, &id).join("events.jsonl");
        drop(h);
        let mut text = std::fs::read_to_string(&log).unwrap();
        text.push_str("{\"seq\": 99, \"ts\": 1, \"type\": \"fin");
        std::fs::write(&log, &text).unwrap();
        let second = Harness::at(data);
        second.resume_scripted(&id, MockProvider::scripted(vec![MockTurn::say("again")])).unwrap();
        second.send_with(&id, "once more", &[], &[], Changes::default()).unwrap();
        until_idle(&second, &id).await;
        let lines = std::fs::read_to_string(&log).unwrap();
        let unreadable = lines.lines().filter(|l| serde_json::from_str::<Event>(l).is_err()).count();
        assert_eq!(unreadable, 1, "only the torn line is lost");
        assert!(second.events(&id).unwrap().iter().any(|e| matches!(&e.kind, EventKind::User { text, .. } if text == "once more")));
    }

    #[tokio::test]
    async fn closing_every_session_leaves_each_one_continuable_and_frees_it() {
        use crate::provider::mock::MockTurn;
        let h = harness("resume-quit");
        h.resume_stand_ins(true);
        let cwd = resume_workspace(&h);
        let id = h.start_scripted(StartRequest { cwd, task: "say hi".into(), ..Default::default() }, MockProvider::scripted(vec![MockTurn::say("hi")])).unwrap().id;
        until_idle(&h, &id).await;
        assert_eq!(h.close_all(), 1);
        assert_eq!((h.close_all(), h.is_live(&id)), (0, false));
        assert_eq!(listed_free(&h).await.status, SessionStatus::Resumable, "the app that quits can be opened again and the session sent a message");
        h.resume_scripted(&id, MockProvider::scripted(vec![])).unwrap();
        assert!(h.is_live(&id));
    }
}
