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
#[cfg(test)]
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
    /// `sage` (default) or `default`, the conventional baseline loop.
    #[serde(default)]
    pub mode: Option<String>,
    /// `auto` (default) or `ask`.
    #[serde(default)]
    pub autonomy: Option<String>,
    #[serde(default)]
    pub max_turns: Option<u32>,
    #[serde(default)]
    pub max_requests: Option<u32>,
    #[serde(default)]
    pub time_budget_secs: Option<u64>,
    /// True when a person is present to answer questions and approvals.
    #[serde(default)]
    pub interactive: bool,
    /// Mechanisms to switch off, for ablation runs (see `Features::NAMES`).
    #[serde(default)]
    pub without: Vec<String>,
    /// Mechanisms to switch on, e.g. Sage's own in the conventional loop (see `Features::NAMES`).
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
    #[serde(default)]
    pub max_turns: Option<u32>,
    #[serde(default)]
    pub time_budget_secs: Option<u64>,
    #[serde(default)]
    pub interactive: Option<bool>,
}

/// The files a message may carry: each one a file that is there, and each once.
fn attachable(files: &[String]) -> Result<Vec<String>, String> {
    let mut kept: Vec<String> = Vec::new();
    for file in files
        .iter()
        .map(|file| file.trim())
        .filter(|file| !file.is_empty())
    {
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
    finished: CancellationToken,
    approvals: Arc<Approvals>,
    /// Released by the worker after its last write and job cleanup.
    lock: Mutex<Option<SessionLock>>,
    storage_warned: AtomicBool,
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
    Goal {
        shown: String,
        command: goal::Command,
        retune: Retune,
    },
}

impl Live {
    fn save_meta(&self, meta: &SessionMeta) -> Result<(), String> {
        let body = serde_json::to_vec_pretty(meta).map_err(|e| e.to_string())?;
        resume::write_atomic(&self.dir.join("meta.json"), &body)
            .map_err(|e| format!("Could not save session metadata: {e}"))
    }

    fn set_status(&self, status: SessionStatus) {
        let result = {
            let mut meta = self.meta.lock().unwrap();
            meta.status = status;
            meta.updated = now_ms();
            self.save_meta(&meta)
        };
        if let Err(problem) = result {
            self.warn_storage(problem);
        }
    }

    fn warn_storage(&self, message: String) {
        if self.storage_warned.swap(true, Ordering::Relaxed) {
            return;
        }
        let mut backlog = self.events.lock().unwrap();
        let warning = Event {
            seq: self.seq.fetch_add(1, Ordering::SeqCst) + 1,
            ts: now_ms(),
            kind: EventKind::Error { message },
        };
        backlog.push(warning.clone());
        let _ = self.tx.send(warning);
    }

    /// Signals the worker to stop accepting messages and end the active run.
    fn shut(&self) {
        self.closed.cancel();
        self.cancel.lock().unwrap().cancel();
    }
}

impl Sink for Live {
    fn emit(&self, kind: EventKind) {
        let transient = kind.transient();
        // Holding the backlog lock across the broadcast lets `subscribe` take a snapshot and a
        // receiver with nothing lost or duplicated in between.
        let mut backlog = self.events.lock().unwrap();
        let event = Event {
            seq: self.seq.fetch_add(1, Ordering::SeqCst) + 1,
            ts: now_ms(),
            kind,
        };
        let mut storage_error = None;
        if !transient {
            if let Some(file) = self.log.lock().unwrap().as_mut() {
                let result = serde_json::to_string(&event)
                    .map_err(std::io::Error::other)
                    .and_then(|line| writeln!(file, "{line}"));
                storage_error = result
                    .err()
                    .map(|e| format!("Could not append to the session log: {e}"));
            }
            match &event.kind {
                EventKind::Stats { stats } => self.meta.lock().unwrap().stats = stats.clone(),
                EventKind::Finish {
                    status, verified, ..
                } => {
                    let mut m = self.meta.lock().unwrap();
                    m.outcome = Some(*status);
                    m.verified = Some(*verified);
                }
                EventKind::Goal {
                    state: state @ (GoalState::Set | GoalState::Cleared | GoalState::Met),
                    text,
                    ..
                } => {
                    let mut meta = self.meta.lock().unwrap();
                    match state {
                        GoalState::Set => meta.goal = Some(Goal::new(text)),
                        GoalState::Cleared => meta.goal = None,
                        _ => {
                            if let Some(goal) = meta.goal.as_mut() {
                                goal.met = true;
                            }
                        }
                    }
                    // A goal that has been met is kept as met even if the app is closed in the middle of the run.
                    storage_error = storage_error.or(self.save_meta(&meta).err());
                }
                _ => {}
            }
            backlog.push(event.clone());
        }
        let _ = self.tx.send(event);
        drop(backlog);
        if let Some(problem) = storage_error {
            self.warn_storage(problem);
        }
    }
}

struct Inner {
    data_dir: PathBuf,
    /// Benchmark harnesses inherit a copy of the user's configuration.
    inherited_config: Mutex<Option<Config>>,
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
        for live in self
            .sessions
            .get_mut()
            .map(|sessions| sessions.drain().map(|(_, live)| live).collect::<Vec<_>>())
            .unwrap_or_default()
        {
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
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}-{}",
        t / 3600,
        (t % 3600) / 60,
        t % 60,
        rand_hex(2)
    )
}

/// The mode a request asks for and the mechanisms it switches on.
fn features_of(req: &StartRequest) -> Result<(Features, Mode), String> {
    let mode = match req.mode.as_deref() {
        Some(name) => Mode::parse(name)
            .ok_or_else(|| format!("unknown mode {name:?}; use {}", Mode::NAMES.join(", ")))?,
        None => Mode::default(),
    };
    let mut features = mode.features();
    let unknown = |name: &String| {
        format!(
            "unknown mechanism {name:?}; known: {}",
            Features::NAMES.join(", ")
        )
    };
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
        Some(condition) if task.is_empty() => Ok(Opening {
            task: goal::task(condition),
            shown: None,
            title: one_line(condition, 72),
            goal: Some(condition.to_string()),
        }),
        Some(condition) => Ok(Opening {
            task: format!("{task}\n\n{}", goal::briefing(condition)),
            shown: Some(task.to_string()),
            title,
            goal: Some(condition.to_string()),
        }),
        None => Ok(Opening {
            task: task.to_string(),
            shown: None,
            title,
            goal: None,
        }),
    }
}

/// Runs a session: takes its messages one at a time and works on each until the session is closed.
struct WorkerExit(Arc<Live>);

impl Drop for WorkerExit {
    fn drop(&mut self) {
        self.0.log.lock().unwrap().take();
        self.0.lock.lock().unwrap().take();
        self.0.finished.cancel();
    }
}

fn spawn_worker(worker: Arc<Live>, mut agent: Agent, mut inbox: mpsc::UnboundedReceiver<Input>) {
    let exit = WorkerExit(worker.clone());
    tokio::spawn(async move {
        let _exit = exit;
        loop {
            // A session that is closed takes no more messages, one that was already waiting included.
            let message = tokio::select! {
                biased;
                _ = worker.closed.cancelled() => None,
                message = inbox.recv() => message,
            };
            let Some(input) = message else { break };
            let cancel = worker.cancel.lock().unwrap().clone();
            if worker.closed.is_cancelled() {
                break;
            }
            match input {
                Input::Message {
                    text,
                    shown,
                    skills,
                    retune,
                    files,
                } => {
                    agent.retune(retune);
                    agent.attach(files);
                    if let Some(shown) = shown {
                        agent.shown_as(shown);
                    }
                    agent.run_with(text, &skills, &cancel).await;
                }
                Input::Goal {
                    shown,
                    command,
                    retune,
                } => {
                    agent.retune(retune);
                    agent.goal_command(&shown, command).await;
                }
            }
            worker.set_status(SessionStatus::Idle);
        }
        agent.sess.jobs.kill_all();
        drop(agent);
    });
}

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .filter(|t| !t.trim().is_empty())
}

/// Writes `text` to `path`; with nothing to write, the file goes.
fn store_text(path: &Path, text: &str) -> Result<(), String> {
    let stored = if text.trim().is_empty() {
        std::fs::remove_file(path).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        })
    } else {
        path.parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(path, text))
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
        Self {
            inner: Arc::new(Inner {
                data_dir,
                inherited_config: Mutex::new(None),
                sessions: Mutex::new(HashMap::new()),
                auto_retry,
                stand_ins: AtomicBool::new(false),
            }),
        }
    }

    /// A harness with this one's keys and defaults that keeps its sessions elsewhere,
    /// so that benchmark runs do not mix with the user's own. It never retries without end: a
    /// benchmark has to finish.
    pub fn with_data_dir(&self, data_dir: PathBuf) -> Self {
        let config = self.config();
        Self {
            inner: Arc::new(Inner {
                data_dir,
                inherited_config: Mutex::new(Some(config)),
                sessions: Mutex::new(HashMap::new()),
                auto_retry: Arc::default(),
                stand_ins: AtomicBool::new(false),
            }),
        }
    }

    fn config_path(&self) -> PathBuf {
        self.inner.data_dir.join("config.json")
    }

    fn config(&self) -> Config {
        let inherited = self.inner.inherited_config.lock().unwrap();
        if let Some(config) = inherited.as_ref() {
            return config.clone();
        }
        let config = Config::load(&self.config_path());
        self.inner
            .auto_retry
            .store(config.auto_retry, Ordering::Relaxed);
        config
    }

    fn update_config<T>(
        &self,
        change: impl FnOnce(&mut Config) -> Result<T, String>,
    ) -> Result<T, String> {
        let path = self.config_path();
        let cannot = |error| format!("could not save the configuration: {error}");
        let mut inherited = self.inner.inherited_config.lock().unwrap();
        std::fs::create_dir_all(&self.inner.data_dir).map_err(cannot)?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .map_err(cannot)?;
        lock.lock().map_err(cannot)?;
        let mut config = inherited.clone().unwrap_or_else(|| Config::load(&path));
        let result = change(&mut config)?;
        config.save(&path).map_err(cannot)?;
        self.inner
            .auto_retry
            .store(config.auto_retry, Ordering::Relaxed);
        if inherited.is_some() {
            *inherited = Some(config);
        }
        Ok(result)
    }

    pub fn state(&self) -> AppState {
        let cfg = self.config();
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
            key_env: if id == CUSTOM {
                "SAGE_API_KEY".into()
            } else {
                String::new()
            },
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
        self.update_config(|cfg| {
            // The file's own endpoint may be given its key before it is written there.
            if provider != CUSTOM && !cfg.knows(provider) {
                return Err(format!("unknown provider {provider:?}"));
            }
            // A key removed is gone from the environment too, for Sage; one given turns it back on.
            if key.trim().is_empty() {
                cfg.keys.remove(provider);
                cfg.env_off.insert(provider.to_string());
            } else {
                cfg.keys
                    .insert(provider.to_string(), key.trim().to_string());
                cfg.env_off.remove(provider);
            }
            Ok(())
        })
    }

    /// Adds an OpenAI-compatible endpoint of the user's own, with its key when it asks for one,
    /// and gives the id it goes by.
    pub fn add_provider(
        &self,
        name: &str,
        base_url: &str,
        models: &[String],
        key: &str,
    ) -> Result<String, String> {
        self.update_config(|cfg| {
            let id = cfg.add_provider(name, base_url, models)?;
            if !key.trim().is_empty() {
                cfg.keys.insert(id.clone(), key.trim().to_string());
            }
            Ok(id)
        })
    }

    /// Takes one of the user's own endpoints away. Sessions already running on it keep running.
    pub fn remove_provider(&self, id: &str) -> Result<(), String> {
        self.update_config(|cfg| cfg.remove_provider(id))
    }

    pub fn set_defaults(
        &self,
        provider: Option<String>,
        model: Option<String>,
        effort: Option<String>,
    ) -> Result<(), String> {
        if let Some(e) = &effort {
            EffortPolicy::parse(e)
                .ok_or_else(|| format!("unknown effort {e:?}; use low, high, max or ratchet"))?;
        }
        self.update_config(|cfg| {
            // A saved model is its provider's: with another provider and no model named, none is kept,
            // or the old provider's model would be asked of the new one.
            if model.is_none()
                && provider
                    .as_ref()
                    .is_some_and(|next| cfg.provider.as_ref() != Some(next))
            {
                cfg.model = None;
            }
            cfg.provider = provider.or(cfg.provider.take());
            cfg.model = model.or(cfg.model.take());
            cfg.effort = effort.or(cfg.effort.take());
            Ok(())
        })
    }

    /// Switches auto-retry: with it, a request the provider turned away for a reason that passes
    /// (a rate limit, an overload, a dropped connection) is made again until it is answered or the
    /// session is stopped, instead of ending the run after a few attempts. Takes effect at once,
    /// in the sessions that are running too.
    pub fn set_auto_retry(&self, on: bool) -> Result<(), String> {
        self.update_config(|cfg| {
            cfg.auto_retry = on;
            Ok(())
        })
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
        Instructions {
            system_custom: custom.is_some(),
            system: custom.unwrap_or_else(|| default.clone()),
            system_default: default,
            global: read_text(&self.global_path()).unwrap_or_default(),
        }
    }

    /// Stores the user's system prompt template. `None`, nothing, or the built-in text itself goes
    /// back to the built-in one.
    pub fn set_system_prompt(&self, template: Option<&str>) -> Result<(), String> {
        let text = template.unwrap_or("");
        store_text(
            &self.system_path(),
            if text.trim() == prompt::default_template() {
                ""
            } else {
                text
            },
        )
    }

    /// Stores (or, with nothing, removes) the user's instructions for every project.
    pub fn set_global_instructions(&self, text: &str) -> Result<(), String> {
        store_text(&self.global_path(), text)
    }

    /// The skills on offer, with the project's own where `cwd` is given.
    pub fn skills(&self, cwd: Option<&str>) -> Vec<SkillInfo> {
        skills::list(
            &self.inner.data_dir,
            cwd.map(str::trim).filter(|c| !c.is_empty()).map(Path::new),
        )
    }

    /// The whole file of a skill, front matter included: what `save_skill` takes.
    pub fn skill_file(&self, name: &str, cwd: Option<&str>) -> Result<String, String> {
        skills::file_text(
            &self.inner.data_dir,
            cwd.map(str::trim).filter(|c| !c.is_empty()).map(Path::new),
            name,
        )
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
        let ws = Workspace::new(Path::new(req.cwd.trim()))
            .map_err(|e| format!("cannot use {:?} as a workspace: {e}", req.cwd))?;
        let (features, mode) = features_of(req)?;
        self.compose_prompt(req, &ws.root, &features, mode == Mode::Plain)
            .map(|(system, _)| system)
    }

    /// The system prompt and the skills of a session. The classic baseline has only its own prompt.
    fn compose_prompt(
        &self,
        req: &StartRequest,
        root: &Path,
        features: &Features,
        classic: bool,
    ) -> Result<(String, Vec<SkillInfo>), String> {
        let picked = skills::resolve(&self.inner.data_dir, Some(root), &req.skills)?;
        if classic {
            return Ok((prompt::SYSTEM_CLASSIC.to_string(), Vec::new()));
        }
        // With scripts switched off, a template about script turns would describe the wrong loop.
        let base = if features.scripts {
            let template = req
                .system_prompt
                .clone()
                .filter(|t| !t.trim().is_empty())
                .or_else(|| read_text(&self.system_path()));
            prompt::render(
                template
                    .as_deref()
                    .unwrap_or_else(|| prompt::default_template()),
                features,
            )
        } else {
            prompt::SYSTEM_CLASSIC.to_string()
        };
        let global = read_text(&self.global_path()).unwrap_or_default();
        let project = if req.project_instructions.unwrap_or(true) {
            prompt::project_instructions(root)
        } else {
            None
        };
        Ok((
            prompt::compose(
                &base,
                &global,
                project.as_ref().map(|(file, text)| (*file, text.as_str())),
                &picked,
            ),
            picked,
        ))
    }

    /// Starts a session on a real provider and begins working on the task.
    pub fn start(&self, req: StartRequest) -> Result<SessionMeta, String> {
        if opening_of(&req)?.task.is_empty() {
            return Err("the task is empty".into());
        }
        let cfg = self.config();
        let resolved = cfg.resolve(req.provider.as_deref(), req.model.as_deref())?;
        let provider = self.client(&resolved)?;
        let effort = req.effort.clone().or(cfg.effort);
        self.launch(req, provider, &resolved.id, &resolved.model, effort)
    }

    /// The way to a model, retrying as the auto-retry setting says.
    fn client(&self, resolved: &Resolved) -> Result<Provider, String> {
        let mut client = OpenAiProvider::new(
            &resolved.base_url,
            &resolved.key,
            &resolved.model,
            resolved.dialect,
        )
        .map_err(|e| e.to_string())?;
        client.keep_retrying = self.inner.auto_retry.clone();
        Ok(Provider::OpenAi(client))
    }

    /// Starts the scripted walkthrough in a throwaway workspace. No key, no network. For tests and
    /// the desktop app's `SAGE_UI_AUTODEMO` screenshot hook: no command offers it to users.
    pub fn demo(&self) -> Result<SessionMeta, String> {
        let root = self.inner.data_dir.join("demo").join(session_id());
        demo::create_workspace(&root)
            .map_err(|e| format!("could not create the demo workspace: {e}"))?;
        let real = crate::util::canonical(&root).map_err(|e| e.to_string())?;
        let provider = Provider::Mock(demo::provider(real, 22));
        // Its stand-in reads only the files it was written for: a survey would put others in front of it.
        let req = StartRequest {
            cwd: root.to_string_lossy().into_owned(),
            task: demo::TASK.into(),
            interactive: true,
            without: vec!["survey".into()],
            ..Default::default()
        };
        self.launch(
            req,
            provider,
            "demo",
            "scripted stand-in",
            Some("high".into()),
        )
    }

    /// Starts a session whose model is a scripted stand-in, for tests and offline benchmark runs.
    /// Its numbers are fabricated and the session is marked as simulated.
    pub fn start_scripted(
        &self,
        req: StartRequest,
        provider: MockProvider,
    ) -> Result<SessionMeta, String> {
        let effort = req.effort.clone();
        self.launch(
            req,
            Provider::Mock(provider),
            "scripted",
            "scripted stand-in",
            effort,
        )
    }

    fn launch(
        &self,
        req: StartRequest,
        provider: Provider,
        provider_id: &str,
        model: &str,
        effort: Option<String>,
    ) -> Result<SessionMeta, String> {
        let ws = Workspace::new(Path::new(req.cwd.trim()))
            .map_err(|e| format!("cannot use {:?} as a workspace: {e}", req.cwd))?;
        let (features, mode) = features_of(&req)?;
        // Read once, here: the system prompt is the cached prefix of every request of the session.
        let (system, picked) =
            self.compose_prompt(&req, &ws.root, &features, mode == Mode::Plain)?;
        let skill_names: Vec<String> = picked.into_iter().map(|s| s.name).collect();
        let mode_label = if mode == Mode::Plain {
            mode.as_str().to_string()
        } else {
            let with = (!req.with.is_empty()).then(|| format!(" with {}", req.with.join(", ")));
            let without =
                (!req.without.is_empty()).then(|| format!(" without {}", req.without.join(", ")));
            format!(
                "{}{}{}",
                mode.as_str(),
                with.unwrap_or_default(),
                without.unwrap_or_default()
            )
        };
        let effort = match effort.as_deref() {
            Some(e) => EffortPolicy::parse(e)
                .ok_or_else(|| format!("unknown effort {e:?}; use low, high, max or ratchet"))?,
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
        let sess = Session::new(ws, dir.clone(), features, prices_for(model))
            .map_err(|e| format!("could not create the session directory: {e}"))?;
        let lock = SessionLock::acquire(&dir).map_err(|e| format!("session {id}: {e}"))?;

        let mut cfg = AgentConfig::new(model);
        cfg.features = features;
        cfg.system = Some(system);
        cfg.effort = effort;
        cfg.autonomy = autonomy;
        cfg.interactive = req.interactive;
        cfg.set_run_limits(req.max_turns, req.time_budget_secs);
        cfg.max_requests = req.max_requests;

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
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("events.jsonl"))
            .map_err(|e| format!("Could not open the session log: {e}"))?;
        let live = Arc::new(Live {
            dir,
            meta: Mutex::new(meta.clone()),
            events: Mutex::new(Vec::new()),
            seq: AtomicU64::new(0),
            tx,
            log: Mutex::new(Some(log)),
            input,
            cancel: Mutex::new(CancellationToken::new()),
            closed: CancellationToken::new(),
            finished: CancellationToken::new(),
            approvals: approvals.clone(),
            lock: Mutex::new(Some(lock)),
            storage_warned: AtomicBool::new(false),
        });
        live.save_meta(&meta)?;
        live.emit(EventKind::SessionStart {
            id: id.clone(),
            title: meta.title.clone(),
            cwd,
            provider: meta.provider.clone(),
            model: meta.model.clone(),
            effort: meta.effort.clone(),
            mode: meta.mode.clone(),
            autonomy: if autonomy == Autonomy::Ask {
                "ask"
            } else {
                "auto"
            }
            .to_string(),
            simulated,
            skills: skill_names,
        });
        self.inner.sessions.lock().unwrap().insert(id, live.clone());

        let sink: Arc<dyn Sink> = live.clone();
        let mut agent = Agent::new(provider, sess, cfg, sink, approvals);
        agent.keep_snapshots(
            live.dir.join(resume::SNAPSHOT),
            Setup {
                mode: mode.as_str().to_string(),
                with: req.with.clone(),
                without: req.without.clone(),
            },
        );
        spawn_worker(live.clone(), agent, inbox);
        let retune = Retune {
            goal: opening.goal,
            ..Default::default()
        };
        live.input
            .send(Input::Message {
                text: opening.task,
                shown: opening.shown,
                skills: Vec::new(),
                retune,
                files,
            })
            .map_err(|_| "the session ended before it started".to_string())?;
        Ok(meta)
    }

    fn live(&self, id: &str) -> Result<Arc<Live>, String> {
        self.inner
            .sessions
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| format!("session {id} is not running in this process"))
    }

    /// Sends a follow-up message to an idle session, switching on the skills named that it does not
    /// have yet. Its system prompt is already sent, so they go in front of this message instead.
    #[cfg(all(test, unix))]
    fn send_with_skills(&self, id: &str, text: &str, skills: &[String]) -> Result<(), String> {
        self.send_with(id, text, skills, &[], Changes::default())
    }

    /// Sends a follow-up, optionally changing the skills, model, effort, or autonomy. The conversation is kept, so another model is sent all of
    /// it. The mode cannot change: the system prompt and the tools of a session are sent already.
    ///
    /// A message that starts with `/goal` is a command (see `goal`): `/goal <condition>` sets the
    /// session's goal and works towards it, `/goal` says what the goal is and `/goal clear` drops
    /// it, the last two with no request to the model.
    ///
    /// A session that is stored and not running in this process is brought back first (see
    /// `resume`), and the message goes to it as to any other.
    pub fn send_with(
        &self,
        id: &str,
        text: &str,
        skills: &[String],
        files: &[String],
        changes: Changes,
    ) -> Result<(), String> {
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
            (
                meta.cwd.clone(),
                meta.simulated,
                meta.provider.clone(),
                meta.model.clone(),
            )
        };
        let mut added = skills::resolve(&self.inner.data_dir, Some(Path::new(&cwd)), skills)?;
        let files = attachable(files)?;
        if matches!(command, Some(goal::Command::Show | goal::Command::Clear))
            && (!added.is_empty() || !files.is_empty())
        {
            return Err(
                "a /goal command carries no skills or files: send them with a message".into(),
            );
        }
        let mut retune = Retune {
            interactive: changes.interactive,
            max_turns: changes.max_turns,
            time_budget_secs: changes.time_budget_secs,
            ..Retune::default()
        };
        if changes
            .provider
            .as_ref()
            .is_some_and(|wanted| *wanted != provider)
            || changes
                .model
                .as_ref()
                .is_some_and(|wanted| *wanted != model)
        {
            if simulated {
                return Err(
                    "this session is played by a stand-in: it has no model to change".into(),
                );
            }
            let resolved = self.config().resolve(
                Some(changes.provider.as_deref().unwrap_or(&provider)),
                changes.model.as_deref(),
            )?;
            retune.provider = Some((self.client(&resolved)?, resolved.id, resolved.model));
        }
        if let Some(effort) = &changes.effort {
            retune.effort = Some(EffortPolicy::parse(effort).ok_or_else(|| {
                format!("unknown effort {effort:?}; use low, high, max or ratchet")
            })?);
        }
        retune.autonomy = match changes.autonomy.as_deref() {
            Some("ask") => Some(Autonomy::Ask),
            Some("auto") => Some(Autonomy::Auto),
            Some(other) => return Err(format!("unknown autonomy {other:?}; use auto or ask")),
            None => None,
        };
        let saved = {
            let mut meta = live.meta.lock().unwrap();
            if live.closed.is_cancelled() {
                return Err("the session has ended".into());
            }
            if meta.status == SessionStatus::Running {
                return Err("the session is still working; stop it first or wait".into());
            }
            // The classic baseline has no skills.
            added.retain(|s| {
                Mode::parse(&meta.mode) != Some(Mode::Plain) && !meta.skills.contains(&s.name)
            });
            meta.skills.extend(added.iter().map(|s| s.name.clone()));
            if let Some((_, provider, model)) = &retune.provider {
                meta.provider = provider.clone();
                meta.model = model.clone();
            }
            if let Some(effort) = retune.effort {
                meta.effort = effort.label();
            }
            // Mark it running now so a second message cannot slip in before the worker picks this one up.
            *live.cancel.lock().unwrap() = CancellationToken::new();
            meta.status = SessionStatus::Running;
            meta.updated = now_ms();
            live.save_meta(&meta)
        };
        if let Err(problem) = saved {
            live.warn_storage(problem);
        }
        let typed = text.trim().to_string();
        let input = match command {
            Some(goal::Command::Set(condition)) => {
                retune.goal = Some(condition.clone());
                Input::Message {
                    text: goal::task(&condition),
                    shown: Some(typed),
                    skills: added,
                    retune,
                    files,
                }
            }
            Some(command) => Input::Goal {
                shown: typed,
                command,
                retune,
            },
            None => Input::Message {
                text: text.to_string(),
                shown: None,
                skills: added,
                retune,
                files,
            },
        };
        live.input
            .send(input)
            .map_err(|_| "the session has ended".to_string())
    }

    /// Stops the run in progress. A session that is only stored has none in this process: that
    /// is not an error, unless another process is running it, which only that one can stop.
    pub fn stop(&self, id: &str) -> Result<(), String> {
        match self.live(id) {
            Ok(live) => live.cancel.lock().unwrap().cancel(),
            Err(not_live) => match self.stored(id) {
                Some(meta) if meta.why_closed.as_deref() == Some("busy") => {
                    return Err(format!(
                        "session {id} is running in another process: stop it there"
                    ));
                }
                Some(_) => {}
                None => return Err(not_live),
            },
        }
        Ok(())
    }

    /// Ends a session for good: stops its current run, kills its background jobs and frees its
    /// memory. The log stays on disk and can still be replayed.
    pub async fn close(&self, id: &str) -> Result<(), String> {
        let live = self
            .inner
            .sessions
            .lock()
            .unwrap()
            .remove(id)
            .ok_or_else(|| format!("session {id} is not running in this process"))?;
        live.shut();
        live.finished.cancelled().await;
        Ok(())
    }

    /// Closes every session this process runs, as the app does when it quits. What each has
    /// stored by then is enough to continue it: a run in progress is ended and its snapshot written.
    pub async fn close_all(&self) -> usize {
        let all: Vec<Arc<Live>> = self
            .inner
            .sessions
            .lock()
            .unwrap()
            .drain()
            .map(|(_, live)| live)
            .collect();
        for live in &all {
            live.shut();
        }
        for live in &all {
            live.finished.cancelled().await;
        }
        all.len()
    }

    /// Gives a session another title: the first line of `title`, as long as a title gets. A
    /// session another process is running is that one's to rename.
    pub fn rename(&self, id: &str, title: &str) -> Result<SessionMeta, String> {
        let title = one_line(title, 72);
        if title.is_empty() {
            return Err("a session needs a title".into());
        }
        if let Ok(live) = self.live(id) {
            let mut meta = live.meta.lock().unwrap();
            if live.closed.is_cancelled() {
                return Err(format!("session {id} is closing"));
            }
            let mut changed = meta.clone();
            changed.title = title;
            live.save_meta(&changed)?;
            *meta = changed;
            return Ok(meta.clone());
        }
        let (dir, lock) = self.unclaimed(id)?;
        let path = dir.join("meta.json");
        // The file as it is, not as this build types it: what a newer build stored in it is kept.
        let mut meta = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .filter(|meta| meta.is_object())
            .ok_or_else(|| format!("no session {id}"))?;
        meta["title"] = title.into();
        let body = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
        resume::write_atomic(&path, body.as_bytes())
            .map_err(|e| format!("could not rename session {id}: {e}"))?;
        drop(lock);
        self.stored(id).ok_or_else(|| format!("no session {id}"))
    }

    /// Deletes a session for good: its conversation, its log and what its commands printed. The
    /// folder it worked in is not touched. A run in progress is stopped first, by whoever asks:
    /// what a run still writes as it ends would come back into a directory being removed.
    pub async fn delete(&self, id: &str) -> Result<(), String> {
        let closing = {
            let mut sessions = self.inner.sessions.lock().unwrap();
            if let Some(live) = sessions.get(id) {
                let meta = live.meta.lock().unwrap();
                if meta.status == SessionStatus::Running {
                    return Err(format!("session {id} is running: stop it first"));
                }
                live.shut();
            }
            sessions.remove(id)
        };
        if let Some(live) = closing {
            live.finished.cancelled().await;
        }
        let (dir, _lock) = self.unclaimed(id)?;
        std::fs::remove_dir_all(&dir).map_err(|e| format!("could not delete session {id}: {e}"))
    }

    /// The directory of a stored session that no process is running.
    fn unclaimed(&self, id: &str) -> Result<(PathBuf, SessionLock), String> {
        let dir = self.session_dir(id)?;
        if !dir.join("meta.json").is_file() {
            return Err(format!("no session {id}"));
        }
        let lock = SessionLock::acquire(&dir)
            .map_err(|error| format!("cannot claim session {id}: {error}"))?;
        if !dir.join("meta.json").is_file() {
            return Err(format!("no session {id}"));
        }
        Ok((dir, lock))
    }

    pub fn approve(&self, id: &str, approval: u64, allow: bool) -> Result<(), String> {
        let Ok(live) = self.live(id) else {
            // What was waiting for an answer died with the process that asked.
            return Err(if self.stored(id).is_some() {
                format!("session {id} is not running: no approval is waiting")
            } else {
                format!("no session {id}")
            });
        };
        if live.approvals.resolve(approval, allow) {
            Ok(())
        } else {
            Err("that approval request is no longer pending".into())
        }
    }

    /// Everything so far plus a live feed, with no gap between the two. A session that is only
    /// stored has no feed: continue it with `send_with` or `resume` first, or read its log with `events`.
    pub fn subscribe(&self, id: &str) -> Result<(Vec<Event>, broadcast::Receiver<Event>), String> {
        let live = match self.live(id) {
            Ok(live) => live,
            Err(not_live) if self.stored(id).is_none() => return Err(not_live),
            Err(_) => {
                return Err(format!(
                    "session {id} is stored, not running: send it a message to continue it, or read its log with `events`"
                ));
            }
        };
        let backlog = live.events.lock().unwrap();
        Ok((backlog.clone(), live.tx.subscribe()))
    }

    /// Stored events of any session, running or past.
    pub fn events(&self, id: &str) -> Result<Vec<Event>, String> {
        self.events_since(id, 0)
    }

    pub fn events_since(&self, id: &str, after: u64) -> Result<Vec<Event>, String> {
        if let Ok(live) = self.live(id) {
            let backlog = live.events.lock().unwrap();
            let start = backlog.partition_point(|event| event.seq <= after);
            return Ok(backlog[start..].to_vec());
        }
        if id.contains(['/', '\\', '.']) {
            return Err("invalid session id".into());
        }
        let path = self
            .inner
            .data_dir
            .join("sessions")
            .join(id)
            .join("events.jsonl");
        let text = std::fs::read_to_string(&path).map_err(|_| format!("no session {id}"))?;
        Ok(text
            .lines()
            .filter_map(|l| serde_json::from_str::<Event>(l).ok())
            .filter(|event| event.seq > after)
            .collect())
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
        let live: HashMap<String, SessionMeta> = self
            .inner
            .sessions
            .lock()
            .unwrap()
            .iter()
            .map(|(id, l)| (id.clone(), l.meta.lock().unwrap().clone()))
            .collect();
        let mut all: Vec<SessionMeta> = live.values().cloned().collect();
        for entry in std::fs::read_dir(self.inner.data_dir.join("sessions"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if live.contains_key(&name) {
                continue;
            }
            if let Some(meta) = std::fs::read_to_string(entry.path().join("meta.json"))
                .ok()
                .and_then(|t| serde_json::from_str::<SessionMeta>(&t).ok())
            {
                all.push(self.as_stored(meta, &entry.path()));
            }
        }
        all.sort_by(|a, b| b.created.cmp(&a.created).then(b.id.cmp(&a.id)));
        all
    }

    /// Full text of an output the model was shown only in part (`out:N`).
    pub fn read_output(&self, id: &str, handle: &str) -> Result<String, String> {
        let n: u32 = handle
            .trim()
            .trim_start_matches("out:")
            .parse()
            .map_err(|_| format!("{handle:?} is not an output handle"))?;
        if id.contains(['/', '\\', '.']) {
            return Err("invalid session id".into());
        }
        let path = self
            .inner
            .data_dir
            .join("sessions")
            .join(id)
            .join("out")
            .join(format!("{n}.log"));
        // Only its start is shown, so only that is read: a command can have printed gigabytes.
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .and_then(|file| file.take(2 * 1024 * 1024).read_to_end(&mut bytes))
            .map_err(|_| format!("no saved output {handle} in session {id}"))?;
        Ok(crate::text::clean(&bytes))
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
        let meta = std::fs::read_to_string(dir.join("meta.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<SessionMeta>(&t).ok())?;
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
        meta.status = if why.is_some() {
            SessionStatus::Closed
        } else {
            SessionStatus::Resumable
        };
        meta.why_closed = why.map(String::from);
        meta
    }

    /// Brings a stored session back to life without sending it anything: its conversation and the
    /// rest of what it kept are restored, the log is reopened for appending and it waits for a
    /// message, idle. A session already running in this process is returned as it is. It is
    /// resumed with the model its meta names, through the current configuration: an error says
    /// so where the key is gone.
    pub fn resume(&self, id: &str) -> Result<SessionMeta, String> {
        self.revive(id, None, &Changes::default())
            .map(|live| live.meta.lock().unwrap().clone())
    }

    /// Lists the sessions a stand-in played as continuable, for a front end that resumes them with
    /// `resume_scripted` (tests, the desktop app's headless hooks). Without it they are read-only.
    pub fn resume_stand_ins(&self, on: bool) {
        self.inner.stand_ins.store(on, Ordering::Relaxed);
    }

    /// `resume` for a session that a stand-in played, with a stand-in to play it from here on. For
    /// tests, and for the desktop app's headless hooks: a real session never needs one.
    pub fn resume_scripted(&self, id: &str, provider: MockProvider) -> Result<SessionMeta, String> {
        self.revive(id, Some(provider), &Changes::default())
            .map(|live| live.meta.lock().unwrap().clone())
    }

    /// The session, live: as it is if this process runs it, else restored from what it stored.
    /// `changes` are the message's own, which may name the provider to use in place of the
    /// stored one: one whose key is gone does not stand in the way of that.
    fn revive(
        &self,
        id: &str,
        stand_in: Option<MockProvider>,
        changes: &Changes,
    ) -> Result<Arc<Live>, String> {
        let dir = self.session_dir(id)?;
        // Held to the end: two messages at once must not both bring the session back.
        let mut sessions = self.inner.sessions.lock().unwrap();
        if let Some(live) = sessions.get(id) {
            return Ok(live.clone());
        }
        let cannot = |why: String| format!("session {id} cannot be continued: {why}");
        if !dir.join("meta.json").is_file() {
            return Err(format!("no session {id}"));
        }
        let lock = SessionLock::acquire(&dir).map_err(cannot)?;
        let mut meta = std::fs::read_to_string(dir.join("meta.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<SessionMeta>(&t).ok())
            .ok_or_else(|| format!("no session {id}"))?;
        if meta.simulated != stand_in.is_some() {
            return Err(cannot(if meta.simulated {
                "it was played by a stand-in, which only a test can bring back".into()
            } else {
                "it was not played by a stand-in".into()
            }));
        }
        let mut snapshot = resume::read(&dir).map_err(cannot)?;
        if !Path::new(&meta.cwd).is_dir() {
            return Err(cannot(format!("its folder {} is gone", meta.cwd)));
        }
        let provider = match stand_in {
            Some(stand_in) => Provider::Mock(stand_in),
            None => {
                let config = self.config();
                match config.resolve(Some(&meta.provider), Some(&meta.model)) {
                    Ok(resolved) => self.client(&resolved)?,
                    // The message names another model: that one is used, and the stored one need not resolve.
                    Err(_) if changes.provider.is_some() || changes.model.is_some() => {
                        let resolved = config.resolve(
                            Some(changes.provider.as_deref().unwrap_or(&meta.provider)),
                            changes.model.as_deref(),
                        )?;
                        self.client(&resolved)?
                    }
                    Err(why) => {
                        return Err(cannot(format!(
                            "its model {} through {} cannot be reached now: {why}",
                            meta.model, meta.provider
                        )));
                    }
                }
            }
        };

        let ws = Workspace::new(Path::new(&meta.cwd))
            .map_err(|e| cannot(format!("cannot use {:?} as a workspace: {e}", meta.cwd)))?;
        // A session stored before Prosage was taken out comes back as Sage, without the
        // mechanisms that went with it.
        let current = |names: &[String]| -> Vec<String> {
            names
                .iter()
                .filter(|n| !Features::RETIRED.contains(&n.replace('-', "_").as_str()))
                .cloned()
                .collect()
        };
        snapshot.setup.with = current(&snapshot.setup.with);
        snapshot.setup.without = current(&snapshot.setup.without);
        if snapshot.setup.mode.eq_ignore_ascii_case("prosage") {
            snapshot.setup.mode = Mode::Sage.as_str().to_string();
        }
        let (features, mode) = features_of(&StartRequest {
            mode: Some(snapshot.setup.mode.clone()),
            with: snapshot.setup.with.clone(),
            without: snapshot.setup.without.clone(),
            ..Default::default()
        })
        .map_err(cannot)?;
        let _ = crate::util::private_dir(&self.inner.data_dir.join("sessions"));
        let sess = Session::new(ws, dir.clone(), features, prices_for(&meta.model))
            .map_err(|e| cannot(format!("could not open the session directory: {e}")))?;
        let mut cfg = AgentConfig::new(&meta.model);
        cfg.features = features;
        let setup = Setup {
            mode: mode.as_str().to_string(),
            with: snapshot.setup.with.clone(),
            without: snapshot.setup.without.clone(),
        };
        let (cut_off, stats) = (snapshot.in_run, snapshot.stats.clone());

        // The log as it was: what a subscriber is shown first, and where the numbering goes on from.
        let log_path = dir.join("events.jsonl");
        let text = match std::fs::read_to_string(&log_path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(cannot(format!("cannot read its log: {error}"))),
        };
        let backlog: Vec<Event> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let last = backlog.iter().map(|e| e.seq).max().unwrap_or(0);
        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| cannot(format!("cannot open its log: {e}")))?;
        // A write cut short leaves a line with no end: the next event must not join it.
        if !text.is_empty() && !text.ends_with('\n') {
            writeln!(log).map_err(|e| cannot(format!("cannot repair its log: {e}")))?;
        }

        meta.status = SessionStatus::Idle;
        meta.why_closed = None;
        meta.stats = stats;
        meta.updated = now_ms();
        let (tx, _) = broadcast::channel(4096);
        let (input, inbox) = mpsc::unbounded_channel::<Input>();
        let approvals = Arc::new(Approvals::default());
        let live = Arc::new(Live {
            dir,
            meta: Mutex::new(meta),
            events: Mutex::new(backlog),
            seq: AtomicU64::new(last),
            tx,
            log: Mutex::new(Some(log)),
            input,
            cancel: Mutex::new(CancellationToken::new()),
            closed: CancellationToken::new(),
            finished: CancellationToken::new(),
            approvals: approvals.clone(),
            lock: Mutex::new(Some(lock)),
            storage_warned: AtomicBool::new(false),
        });
        let goal = live.meta.lock().unwrap().goal.clone();
        let sink: Arc<dyn Sink> = live.clone();
        let (mut agent, answered) =
            Agent::restore(provider, sess, cfg, sink, approvals, snapshot, goal);
        agent.keep_snapshots(live.dir.join(resume::SNAPSHOT), setup);
        live.save_meta(&live.meta.lock().unwrap())?;
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
mod tests;

#[cfg(test)]
mod persistence_tests {
    use super::*;

    fn scratch() -> PathBuf {
        std::env::temp_dir().join(format!("sage-persistence-{}", rand_hex(8)))
    }

    #[test]
    fn two_harnesses_keep_each_others_configuration_changes() {
        let data = scratch();
        let a = Harness::at(data.clone());
        let b = Harness::at(data.clone());
        a.set_key("openai", "test-a").unwrap();
        b.set_key("deepseek", "test-b").unwrap();
        a.set_auto_retry(true).unwrap();
        let config = Config::load(&a.config_path());
        assert_eq!(
            config.keys.get("openai").map(String::as_str),
            Some("test-a")
        );
        assert_eq!(
            config.keys.get("deepseek").map(String::as_str),
            Some("test-b")
        );
        assert!(b.state().auto_retry);
        std::fs::remove_dir_all(data).unwrap();
    }

    #[tokio::test]
    async fn stopping_a_queued_run_cancels_it_before_the_first_request() {
        let data = scratch();
        let cwd = data.join("work");
        std::fs::create_dir_all(&cwd).unwrap();
        let harness = Harness::at(data.clone());
        let provider = MockProvider::new(vec![Box::new(|_| {
            panic!("queued run contacted its provider")
        })]);
        let meta = harness
            .start_scripted(
                StartRequest {
                    cwd: cwd.to_string_lossy().into_owned(),
                    task: "Say hello".into(),
                    without: vec!["survey".into()],
                    ..Default::default()
                },
                provider,
            )
            .unwrap();
        harness.stop(&meta.id).unwrap();
        let (_, mut feed) = harness.subscribe(&meta.id).unwrap();
        let status = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let EventKind::Finish { status, .. } = feed.recv().await.unwrap().kind {
                    break status;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(status, FinishStatus::Stopped);
        assert_eq!(harness.close_all().await, 1);
        let dir = harness.session_dir(&meta.id).unwrap();
        assert!(!SessionLock::held(&dir));
        assert!(!resume::read(&dir).unwrap().in_run);
        harness.delete(&meta.id).await.unwrap();
        std::fs::remove_dir_all(data).unwrap();
    }
}
