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
use crate::script::{Approvals, Autonomy};
use crate::skills::{self, SkillInfo};
use crate::state::{Features, Session};
use crate::util::{civil_date, home_dir, now_ms, one_line, rand_hex};
use crate::workspace::Workspace;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
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
    /// Loaded from disk: viewable, not resumable.
    Closed,
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
    /// The goal the session works towards, if one was set, and whether it has been found met.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<Goal>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct StartRequest {
    pub cwd: String,
    /// What to do. `/goal <condition>` here sets the goal and makes it the task.
    #[serde(default)]
    pub task: String,
    /// A completion condition: the session works until a check of the harness's own agrees that
    /// it holds. Given without a task, it is the task.
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// `low`, `high`, `max`, `off`, or `ratchet[-low|-high]`.
    #[serde(default)]
    pub effort: Option<String>,
    /// `presage` (default) or `classic`, the conventional baseline loop.
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
}

/// What a session's worker is handed: a user message with the skills to switch on, what else
/// changes with it and the files attached to it. With no message, only what changes (`/goal clear`).
struct Input {
    text: Option<String>,
    skills: Vec<SkillInfo>,
    retune: Retune,
    files: Vec<String>,
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

    /// `/goal`: says in the log what the goal is and whether it is met. An empty one is none.
    fn report_goal(&self) {
        let goal = self.meta.lock().unwrap().goal.clone().unwrap_or_default();
        self.emit(EventKind::Goal { goal: goal.text, state: GoalState::Status, met: goal.met, reason: String::new() });
    }

    /// `/goal clear`: the session has no goal from here on. Not while a run is under way, which
    /// would go on with the goal it started with.
    fn clear_goal(&self) -> Result<(), String> {
        let goal = {
            let meta = self.meta.lock().unwrap();
            if meta.status == SessionStatus::Running {
                return Err("the session is still working; stop it first or wait".into());
            }
            meta.goal.clone()
        };
        let Some(goal) = goal else {
            self.report_goal();
            return Ok(());
        };
        self.input.send(Input { text: None, skills: Vec::new(), retune: Retune { goal: Some(None), ..Default::default() }, files: Vec::new() }).map_err(|_| "the session has ended".to_string())?;
        self.emit(EventKind::Goal { goal: goal.text, state: GoalState::Cleared, met: goal.met, reason: String::new() });
        self.save_meta();
        Ok(())
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
                // The stored goal follows the log: set, found met, cleared.
                EventKind::Goal { goal, state, .. } => {
                    let mut m = self.meta.lock().unwrap();
                    match state {
                        GoalState::Set => m.goal = Some(Goal::new(goal)),
                        GoalState::Met => m.goal.iter_mut().for_each(|g| g.met = true),
                        GoalState::Cleared => m.goal = None,
                        GoalState::Status | GoalState::NotMet | GoalState::Unchecked | GoalState::Open => {}
                    }
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

/// The mechanisms a request switches on, and whether it asks for the classic baseline.
fn features_of(req: &StartRequest) -> Result<(Features, bool), String> {
    let classic = matches!(req.mode.as_deref(), Some("classic"));
    let mut features = if classic { Features::classic() } else { Features::presage() };
    let unknown = |name: &String| format!("unknown mechanism {name:?}; known: {}", Features::NAMES.join(", "));
    for name in &req.with {
        features = features.with(name).ok_or_else(|| unknown(name))?;
    }
    for name in &req.without {
        features = features.without(name).ok_or_else(|| unknown(name))?;
    }
    Ok((features, classic))
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
        Self { inner: Arc::new(Inner { data_dir, config: Mutex::new(config), sessions: Mutex::new(HashMap::new()), auto_retry }) }
    }

    /// A harness with this one's keys and defaults that keeps its sessions elsewhere,
    /// so that benchmark runs do not mix with the user's own. It never retries without end: a
    /// benchmark has to finish.
    pub fn with_data_dir(&self, data_dir: PathBuf) -> Self {
        let config = self.inner.config.lock().unwrap().clone();
        Self { inner: Arc::new(Inner { data_dir, config: Mutex::new(config), sessions: Mutex::new(HashMap::new()), auto_retry: Arc::default() }) }
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
            key_env: if id == CUSTOM { "PRESAGE_API_KEY".into() } else { String::new() },
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
        if key.trim().is_empty() {
            cfg.keys.remove(provider);
        } else {
            cfg.keys.insert(provider.to_string(), key.trim().to_string());
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
        let (features, classic) = features_of(req)?;
        self.compose_prompt(req, &ws.root, &features, classic).map(|(system, _)| system)
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
        // A goal with no task is the task.
        if req.task.trim().is_empty() && req.goal.as_deref().is_none_or(|goal| goal.trim().is_empty()) {
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
        let (features, classic) = features_of(&req)?;
        // The `/goal` command is read here, so that every front end takes it the same way.
        let (task, goal) = goal::at_start(&req.task, req.goal.as_deref())?;
        // Read once, here: the system prompt is the cached prefix of every request of the session.
        let (system, picked) = self.compose_prompt(&req, &ws.root, &features, classic)?;
        let skill_names: Vec<String> = picked.into_iter().map(|s| s.name).collect();
        let mode = if classic {
            "classic".to_string()
        } else {
            let with = (!req.with.is_empty()).then(|| format!(" with {}", req.with.join(", ")));
            let without = (!req.without.is_empty()).then(|| format!(" without {}", req.without.join(", ")));
            format!("presage{}{}", with.unwrap_or_default(), without.unwrap_or_default())
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

        let id = session_id();
        let dir = self.inner.data_dir.join("sessions").join(&id);
        let simulated = provider.simulated();
        let cwd = ws.root.to_string_lossy().into_owned();
        let sess = Session::new(ws, dir.clone(), features, prices_for(model)).map_err(|e| format!("could not create the session directory: {e}"))?;

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

        let now = now_ms();
        let meta = SessionMeta {
            id: id.clone(),
            // A session started with `/goal <condition>` goes by its condition.
            title: one_line(if goal::parse(&task).is_some() { goal.as_deref().unwrap_or(&task) } else { &task }, 72),
            cwd: cwd.clone(),
            provider: provider_id.to_string(),
            model: model.to_string(),
            mode,
            effort: effort.label(),
            simulated,
            created: now,
            updated: now,
            status: SessionStatus::Running,
            outcome: None,
            verified: None,
            stats: Stats::default(),
            skills: skill_names.clone(),
            goal: goal.as_deref().map(Goal::new),
        };
        let (tx, _) = broadcast::channel(4096);
        let (input, mut inbox) = mpsc::unbounded_channel::<Input>();
        let approvals = Arc::new(Approvals::default());
        let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("events.jsonl")).ok();
        let live = Arc::new(Live { dir, meta: Mutex::new(meta.clone()), events: Mutex::new(Vec::new()), seq: AtomicU64::new(0), tx, log: Mutex::new(log), input, cancel: Mutex::new(CancellationToken::new()), closed: CancellationToken::new(), approvals: approvals.clone() });
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
        let worker = live.clone();
        tokio::spawn(async move {
            loop {
                let message = tokio::select! {
                    message = inbox.recv() => message,
                    _ = worker.closed.cancelled() => None,
                };
                let Some(input) = message else { break };
                agent.retune(input.retune);
                // What only changes the session asks nothing of the model and starts no run.
                let Some(text) = input.text else { continue };
                let cancel = CancellationToken::new();
                *worker.cancel.lock().unwrap() = cancel.clone();
                worker.set_status(SessionStatus::Running);
                agent.attach(input.files);
                agent.run_with(text, &input.skills, &cancel).await;
                worker.set_status(SessionStatus::Idle);
            }
            agent.sess.jobs.kill_all();
        });
        let retune = Retune { goal: goal.map(Some), ..Default::default() };
        live.input.send(Input { text: Some(task), skills: Vec::new(), retune, files }).map_err(|_| "the session ended before it started".to_string())?;
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
    pub fn send_with(&self, id: &str, text: &str, skills: &[String], files: &[String], changes: Changes) -> Result<(), String> {
        let live = self.live(id)?;
        if text.trim().is_empty() {
            return Err("the message is empty".into());
        }
        // `/goal` is a command, read here so that every front end takes it the same way. Asking
        // what the goal is and clearing it are answered at once, with no request to the model.
        let command = goal::parse(text);
        match command {
            Some(goal::Command::Show) => {
                live.report_goal();
                return Ok(());
            }
            Some(goal::Command::Clear) => return live.clear_goal(),
            Some(goal::Command::Set(_)) | None => {}
        }
        let (cwd, simulated, provider, model) = {
            let meta = live.meta.lock().unwrap();
            (meta.cwd.clone(), meta.simulated, meta.provider.clone(), meta.model.clone())
        };
        let mut added = skills::resolve(&self.inner.data_dir, Some(Path::new(&cwd)), skills)?;
        let files = attachable(files)?;
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
        // `/goal <condition>` sets the goal and is the message: the model is sent the goal as its task.
        if let Some(goal::Command::Set(condition)) = command {
            retune.goal = Some(Some(goal::checked(&condition)?));
        }
        {
            let mut meta = live.meta.lock().unwrap();
            if meta.status == SessionStatus::Running {
                return Err("the session is still working; stop it first or wait".into());
            }
            if let Some(Some(goal)) = &retune.goal {
                meta.goal = Some(Goal::new(goal));
            }
            // The classic baseline has no skills.
            added.retain(|s| meta.mode != "classic" && !meta.skills.contains(&s.name));
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
        live.input.send(Input { text: Some(text.to_string()), skills: added, retune, files }).map_err(|_| "the session has ended".to_string())
    }

    pub fn stop(&self, id: &str) -> Result<(), String> {
        self.live(id)?.cancel.lock().unwrap().cancel();
        Ok(())
    }

    /// Ends a session for good: stops its current run, kills its background jobs and frees its
    /// memory. The log stays on disk and can still be replayed.
    pub fn close(&self, id: &str) -> Result<(), String> {
        let live = self.inner.sessions.lock().unwrap().remove(id).ok_or_else(|| format!("session {id} is not running in this process"))?;
        live.cancel.lock().unwrap().cancel();
        live.closed.cancel();
        Ok(())
    }

    pub fn approve(&self, id: &str, approval: u64, allow: bool) -> Result<(), String> {
        if self.live(id)?.approvals.resolve(approval, allow) { Ok(()) } else { Err("that approval request is no longer pending".into()) }
    }

    /// Everything so far plus a live feed, with no gap between the two.
    pub fn subscribe(&self, id: &str) -> Result<(Vec<Event>, broadcast::Receiver<Event>), String> {
        let live = self.live(id)?;
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

    pub fn meta(&self, id: &str) -> Result<SessionMeta, String> {
        self.live(id).map(|l| l.meta.lock().unwrap().clone())
    }

    /// All sessions, newest first. Ones from earlier runs of the app are listed as closed.
    pub fn list(&self) -> Vec<SessionMeta> {
        let live: HashMap<String, SessionMeta> = self.inner.sessions.lock().unwrap().iter().map(|(id, l)| (id.clone(), l.meta.lock().unwrap().clone())).collect();
        let mut all: Vec<SessionMeta> = live.values().cloned().collect();
        for entry in std::fs::read_dir(self.inner.data_dir.join("sessions")).into_iter().flatten().flatten() {
            let Ok(name) = entry.file_name().into_string() else { continue };
            if live.contains_key(&name) {
                continue;
            }
            if let Some(mut meta) = std::fs::read_to_string(entry.path().join("meta.json")).ok().and_then(|t| serde_json::from_str::<SessionMeta>(&t).ok()) {
                meta.status = SessionStatus::Closed;
                all.push(meta);
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
        let bytes = std::fs::read(&path).map_err(|_| format!("no saved output {handle} in session {id}"))?;
        Ok(crate::text::clean(&bytes[..bytes.len().min(2 * 1024 * 1024)]))
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

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn harness(name: &str) -> Harness {
        Harness::at(std::env::temp_dir().join(format!("presage-harness-{name}-{}", rand_hex(4))))
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
        assert_eq!((meta.mode.as_str(), meta.simulated), ("presage without auto-watch, survey", true));
        let done = until_idle(&h, &meta.id).await;
        assert_eq!((done.outcome, done.verified), (Some(FinishStatus::Done), Some(true)));

        // Closing frees the session; its log can still be replayed.
        h.close(&meta.id).unwrap();
        assert!(h.meta(&meta.id).is_err() && h.close(&meta.id).is_err());
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
        assert!(fresh.system_default.starts_with("You are Presage") && fresh.system_default.contains("{watch}"));

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
        std::fs::create_dir_all(ws.join(".presage/skills")).unwrap();
        std::fs::write(ws.join("AGENTS.md"), "Run make test.\n").unwrap();
        std::fs::write(ws.join(".presage/skills/house.md"), "---\ntitle: House rules\n---\nNo globals.\n").unwrap();
        h.set_global_instructions("Use tabs.").unwrap();
        let cwd = ws.to_string_lossy().into_owned();
        let req = || StartRequest { cwd: cwd.clone(), task: "hello".into(), interactive: true, skills: vec!["security".into(), "house".into()], ..Default::default() };
        let skill = |name: &str| h.skills(Some(&cwd)).into_iter().find(|s| s.name == name).unwrap();

        // Template, the user's instructions, the project's, then the skills in the order asked for.
        let built_in = prompt::system_prompt(&Features::presage());
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
        // `with` takes a mechanism by name; what has been retired is not one any more.
        assert_eq!(h.system_prompt(&StartRequest { with: vec!["survey".into()], ..bare.clone() }).unwrap(), h.system_prompt(&bare).unwrap());
        assert!(h.system_prompt(&StartRequest { with: vec!["review".into()], ..bare.clone() }).unwrap_err().contains("unknown mechanism"));
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
        let meta = SessionMeta { id: "s".into(), title: "t".into(), cwd: "/".into(), provider: "p".into(), model: "m".into(), mode: "presage".into(), effort: "high".into(), simulated: false, created: 1, updated: 2, status: SessionStatus::Idle, outcome: None, verified: None, stats: Stats::default(), skills: vec!["ui".into()], goal: None };
        let mut old = serde_json::to_value(&meta).unwrap();
        assert_eq!(old.as_object_mut().unwrap().remove("skills"), Some(serde_json::json!(["ui"])));
        assert!(serde_json::from_value::<SessionMeta>(old).unwrap().skills.is_empty());

        let start = r#"{"seq":1,"ts":1,"type":"session_start","id":"s","title":"t","cwd":"/","provider":"p","model":"m","effort":"high","mode":"presage","autonomy":"auto","simulated":false}"#;
        assert!(matches!(serde_json::from_str::<Event>(start).unwrap().kind, EventKind::SessionStart { skills, .. } if skills.is_empty()));
        let user = r#"{"seq":2,"ts":1,"type":"user","text":"hi"}"#;
        let event = serde_json::from_str::<Event>(user).unwrap();
        assert!(matches!(&event.kind, EventKind::User { skills, .. } if skills.is_empty()));
        assert_eq!(serde_json::to_string(&event).unwrap(), user);

        let req: StartRequest = serde_json::from_str(r#"{"cwd":"/tmp","task":"x"}"#).unwrap();
        assert!(req.skills.is_empty() && req.system_prompt.is_none() && req.project_instructions.is_none());
    }

    /// A stand-in that keeps the last message of every request it is sent, the goal's checks among them.
    fn recorded(turns: Vec<crate::provider::mock::MockTurn>) -> (MockProvider, Arc<Mutex<Vec<String>>>) {
        use crate::provider::mock::TurnFn;
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let turn = |reply: crate::provider::mock::MockTurn| {
            let seen = seen.clone();
            Box::new(move |messages: &[crate::provider::Message]| {
                seen.lock().unwrap().push(messages.last().map(|m| m.content.clone()).unwrap_or_default());
                reply.clone()
            }) as TurnFn
        };
        (MockProvider::new(turns.into_iter().map(turn).collect()), seen.clone())
    }

    /// What the log says about the goal, in order: the state, the goal, whether it is met, the reason.
    fn goal_log(h: &Harness, id: &str) -> Vec<(GoalState, String, bool, String)> {
        h.events(id).unwrap().iter().filter_map(|e| if let EventKind::Goal { goal, state, met, reason } = &e.kind { Some((*state, goal.clone(), *met, reason.clone())) } else { None }).collect()
    }

    /// The session as a later launch of the app lists it, once it is on disk as `ready` wants it:
    /// `until_idle` goes by what is in memory, which is written to disk a moment later.
    async fn stored(h: &Harness, id: &str, ready: impl Fn(&SessionMeta) -> bool) -> SessionMeta {
        for _ in 0..400 {
            let later = Harness::at(h.inner.data_dir.clone());
            if let Some(meta) = later.list().into_iter().find(|m| m.id == id && ready(m)) {
                return meta;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("session {id} was not stored as expected");
    }

    fn workspace(h: &Harness) -> (PathBuf, String) {
        let ws = h.inner.data_dir.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let cwd = ws.to_string_lossy().into_owned();
        (ws, cwd)
    }

    #[tokio::test]
    async fn a_goal_given_at_the_start_holds_the_session_until_it_is_met_and_is_stored() {
        use crate::provider::mock::MockTurn;
        use serde_json::json;
        let h = harness("goal-start");
        let (ws, cwd) = workspace(&h);
        let write = |path: &str| MockTurn::calls("", vec![("write", json!({"path": path, "content": "x"})), ("shell", json!({"cmd": format!("test -f {path}"), "expect_exit": "0"})), ("finish", json!({"summary": format!("wrote {path}")}))]);
        let (provider, seen) = recorded(vec![write("a.txt"), MockTurn::say("NOT MET: b.txt is missing"), write("b.txt"), MockTurn::say("MET")]);
        let req = StartRequest { cwd: cwd.clone(), task: "Write the files.".into(), goal: Some(" a.txt and b.txt exist ".into()), ..Default::default() };
        let meta = h.start_scripted(req, provider).unwrap();
        assert_eq!((meta.title.as_str(), meta.goal.as_ref().map(|g| (g.text.as_str(), g.met))), ("Write the files.", Some(("a.txt and b.txt exist", false))), "known from the start");

        let done = until_idle(&h, &meta.id).await;
        assert_eq!((done.outcome, done.verified, done.goal.as_ref().map(|g| g.met)), (Some(FinishStatus::Done), Some(true), Some(true)));
        assert!(ws.join("b.txt").is_file(), "refused once, the session went on");
        // Two replies and two checks, all of them counted.
        assert_eq!((seen.lock().unwrap().len(), done.stats.turns), (4, 4));
        let log = goal_log(&h, &meta.id);
        assert_eq!(log.iter().map(|(state, ..)| *state).collect::<Vec<_>>(), [GoalState::Set, GoalState::NotMet, GoalState::Met]);
        assert_eq!(log[1], (GoalState::NotMet, "a.txt and b.txt exist".into(), false, "b.txt is missing".into()));
        assert!(log[2].2, "met, as of that event");

        // It is kept with the session: a later launch lists it, met.
        let later = stored(&h, &meta.id, |m| m.goal.as_ref().is_some_and(|g| g.met)).await;
        assert_eq!(later.goal.as_ref().map(|g| g.text.as_str()), Some("a.txt and b.txt exist"));

        // With no task, the goal is the task, in the form the command has.
        let (provider, seen) = recorded(vec![MockTurn::calls("", vec![("finish", json!({"summary": "they are there"}))]), MockTurn::say("MET")]);
        let meta = h.start_scripted(StartRequest { cwd: cwd.clone(), goal: Some("a.txt and b.txt exist".into()), ..Default::default() }, provider).unwrap();
        assert_eq!(meta.title, "a.txt and b.txt exist");
        until_idle(&h, &meta.id).await;
        assert!(matches!(&h.events(&meta.id).unwrap()[1].kind, EventKind::User { text, .. } if text == "/goal a.txt and b.txt exist"));
        assert!(seen.lock().unwrap()[0].contains("<task>\nWork until this goal is met:\n<goal>\na.txt and b.txt exist\n</goal>\n"), "{}", seen.lock().unwrap()[0]);

        // A start that states no goal, or two, or none where one is needed, is refused.
        let bad = |task: &str, goal: Option<&str>| h.start_scripted(StartRequest { cwd: cwd.clone(), task: task.into(), goal: goal.map(String::from), ..Default::default() }, MockProvider::scripted(vec![])).unwrap_err();
        assert!(bad("/goal", None).contains("no goal yet") && bad("/goal clear", None).contains("no goal yet"));
        assert!(bad("/goal a", Some("b")).contains("given twice"));
        assert_eq!(h.start(StartRequest { cwd: cwd.clone(), goal: Some("  ".into()), ..Default::default() }).unwrap_err(), "the task is empty");
        assert_eq!(h.list().len(), 2, "nothing was started by any of them");
    }

    #[tokio::test]
    async fn goal_commands_sent_as_messages_set_report_and_clear_the_goal() {
        use crate::provider::mock::MockTurn;
        use serde_json::json;
        let h = harness("goal-commands");
        let (_, cwd) = workspace(&h);
        let finish = |summary: &str| MockTurn::calls("", vec![("finish", json!({"summary": summary}))]);
        let (provider, seen) = recorded(vec![
            MockTurn::say("hi"),
            finish("nothing to do"),
            MockTurn::say("NOT MET: a.txt is not there"),
            MockTurn::calls("", vec![("write", json!({"path": "a.txt", "content": "x"})), ("shell", json!({"cmd": "test -f a.txt", "expect_exit": "0"})), ("finish", json!({"summary": "wrote a.txt"}))]),
            MockTurn::say("MET"),
            finish("ok"),
            finish("done"),
            MockTurn::say("NOT MET: there is no b.txt"),
            MockTurn::say("I cannot."),
            finish("fine"),
        ]);
        let asked = || seen.lock().unwrap().len();
        let meta = h.start_scripted(StartRequest { cwd, task: "hello".into(), interactive: true, ..Default::default() }, provider).unwrap();
        let id = meta.id.as_str();
        assert!(meta.goal.is_none() && until_idle(&h, id).await.goal.is_none());
        let send = |text: &str| h.send_with_skills(id, text, &[]);
        let goal = || h.meta(id).unwrap().goal.map(|g| (g.text, g.met));
        let last = || goal_log(&h, id).pop().unwrap();

        // `/goal` alone says what the goal is. No request is made, and the session stays as it was.
        send("/goal").unwrap();
        assert_eq!(last(), (GoalState::Status, String::new(), false, String::new()), "none yet");
        assert_eq!((asked(), h.meta(id).unwrap().status), (1, SessionStatus::Idle));

        // `/goal <condition>` sets it and starts work on it: the model is sent the goal as its task.
        send("  /goal a.txt exists\n").unwrap();
        assert_eq!(goal(), Some(("a.txt exists".into(), false)), "set with the message");
        let done = until_idle(&h, id).await;
        assert_eq!((done.outcome, done.goal.as_ref().map(|g| g.met), asked()), (Some(FinishStatus::Done), Some(true), 5));
        let events = h.events(id).unwrap();
        let typed = events.iter().position(|e| matches!(&e.kind, EventKind::User { text, .. } if text == "/goal a.txt exists")).expect("the message as it was typed");
        assert!(matches!(&events[typed + 1].kind, EventKind::Goal { state: GoalState::Set, .. }));
        assert!(seen.lock().unwrap()[1].starts_with("Work until this goal is met:\n<goal>\na.txt exists\n</goal>\n"), "{}", seen.lock().unwrap()[1]);
        assert!(seen.lock().unwrap()[2].starts_with("<goal>\na.txt exists\n</goal>\n<closing_message>\nnothing to do\n"), "the check is asked about the goal");
        assert_eq!(goal_log(&h, id).iter().map(|(state, ..)| *state).collect::<Vec<_>>(), [GoalState::Status, GoalState::Set, GoalState::NotMet, GoalState::Met]);

        // Met, it is reported as met and gates nothing more.
        send("/goal").unwrap();
        assert_eq!(last(), (GoalState::Status, "a.txt exists".into(), true, String::new()));
        send("more").unwrap();
        until_idle(&h, id).await;
        assert_eq!((asked(), goal()), (6, Some(("a.txt exists".into(), true))), "one reply, no check");

        // Another goal takes its place, and stays open when the model hands the turn back.
        send("/goal b.txt exists").unwrap();
        let open = until_idle(&h, id).await;
        assert_eq!((open.outcome, asked(), goal()), (Some(FinishStatus::Yielded), 9, Some(("b.txt exists".into(), false))));

        // `/goal clear` removes it, again with no request; a second one finds nothing to clear.
        send("/goal clear").unwrap();
        assert_eq!(last(), (GoalState::Cleared, "b.txt exists".into(), false, String::new()));
        assert_eq!((goal(), asked(), h.meta(id).unwrap().status), (None, 9, SessionStatus::Idle));
        send("/goal clear").unwrap();
        assert_eq!(last(), (GoalState::Status, String::new(), false, String::new()));
        send("again").unwrap();
        let done = until_idle(&h, id).await;
        assert_eq!((done.outcome, asked(), done.goal), (Some(FinishStatus::Done), 10, None), "nothing checks this `finish`");

        // The commands are no messages: the model was sent none of them.
        assert!(seen.lock().unwrap().iter().all(|m| !m.trim_start().starts_with("/goal")));
        assert_eq!(h.events(id).unwrap().iter().filter(|e| matches!(e.kind, EventKind::User { .. })).count(), 5);
        // A goal that is too long is refused before anything is sent.
        assert!(send(&format!("/goal {}", "x".repeat(goal::MAX_GOAL_CHARS + 1))).unwrap_err().contains("at most 4000"));
        assert_eq!((goal(), h.meta(id).unwrap().status), (None, SessionStatus::Idle));

        // Cleared is stored as cleared.
        assert!(stored(&h, id, |m| m.stats.turns == 10).await.goal.is_none());
    }

    #[test]
    fn a_log_from_before_memory_and_review_were_retired_still_opens() {
        let h = harness("retired");
        let id = "20261101-101010-ab12";
        let dir = h.inner.data_dir.join("sessions").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let stats = r#"{"turns":2,"steps":3,"skipped":0,"surprises":0,"predictions":1,"predictions_held":1,"prompt_tokens":10,"cached_tokens":0,"output_tokens":5,"reasoning_tokens":0,"context_tokens":10,"cost":0.0,"priced":false,"hidden_bytes":0,"repairs":0,"retries":0,"cache_misses":0,"cache_busts":0,"lessons_learned":1,"elapsed_ms":100,"model_ms":50,"tool_ms":10}"#;
        let meta = format!(r#"{{"id":"{id}","title":"Fix it","cwd":"/tmp/x","provider":"logfare","model":"glm-5.3","mode":"presage with review","effort":"ratchet-low","simulated":false,"created":5,"updated":9,"status":"idle","outcome":"done","verified":true,"stats":{stats},"skills":[]}}"#);
        std::fs::write(dir.join("meta.json"), meta).unwrap();
        let lines = [
            format!(r#"{{"seq":1,"ts":1,"type":"session_start","id":"{id}","title":"Fix it","cwd":"/tmp/x","provider":"logfare","model":"glm-5.3","effort":"ratchet-low","mode":"presage with review","autonomy":"auto","simulated":false,"skills":[]}}"#),
            r#"{"seq":2,"ts":2,"type":"user","text":"Fix it"}"#.to_string(),
            r#"{"seq":3,"ts":3,"type":"lesson","learned":false,"text":"`pytest -q` fails here (not found) → `uv run pytest -q` works"}"#.to_string(),
            r#"{"seq":4,"ts":4,"type":"turn_start","turn":1,"effort":"low"}"#.to_string(),
            r#"{"seq":5,"ts":5,"type":"step_end","turn":1,"idx":0,"call_id":"c1","tool":"finish","status":"ok","observation":"[review] Not accepted yet: one review pass first.","ms":1,"hidden_bytes":0,"violations":[],"explicit":false}"#.to_string(),
            r#"{"seq":6,"ts":6,"type":"lesson","learned":true,"text":"`python` fails here → `python3` works"}"#.to_string(),
            r#"{"seq":7,"ts":7,"type":"review","turn":1,"text":"one review pass"}"#.to_string(),
            format!(r#"{{"seq":8,"ts":8,"type":"stats","stats":{stats}}}"#),
            r#"{"seq":9,"ts":9,"type":"finish","status":"done","summary":"Fixed.","verified":true,"evidence":"verified by watch tests"}"#.to_string(),
        ];
        std::fs::write(dir.join("events.jsonl"), lines.join("\n") + "\n").unwrap();

        // It is listed as it was, with the mode it ran in and no goal.
        let listed = h.list();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].status, listed[0].mode.as_str(), listed[0].stats.turns, listed[0].outcome, listed[0].goal.clone()), (SessionStatus::Closed, "presage with review", 2, Some(FinishStatus::Done), None));
        // What the retired mechanisms wrote is passed over; everything else replays in order.
        let events = h.events(id).unwrap();
        assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), [1, 2, 4, 5, 8, 9]);
        assert!(matches!(&events[3].kind, EventKind::StepEnd { observation, .. } if observation.starts_with("[review] Not accepted yet")), "a step is kept with what it said then");
        assert!(matches!(&events[4].kind, EventKind::Stats { stats } if stats.turns == 2 && stats.model_ms == 50));
        assert!(matches!(&events[5].kind, EventKind::Finish { status: FinishStatus::Done, verified: true, .. }));
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
}
