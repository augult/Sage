//! The public facade: everything a front end (desktop app, CLI) needs, and nothing else.
//!
//! A session is a long-lived task that owns its agent. Events are appended to `events.jsonl`,
//! kept in memory for late subscribers, and broadcast live.

use crate::agent::{Agent, AgentConfig, EffortPolicy};
use crate::config::{Config, PROVIDERS, data_dir};
use crate::demo;
use crate::economy::prices_for;
use crate::events::{Event, EventKind, FinishStatus, Sink, Stats};
use crate::lessons::LessonBook;
use crate::prompt;
use crate::provider::Provider;
use crate::provider::mock::MockProvider;
use crate::provider::openai::OpenAiProvider;
use crate::script::{Approvals, Autonomy};
use crate::skills::{self, SkillInfo};
use crate::state::{Features, Session};
use crate::util::{civil_date, fnv64, home_dir, now_ms, one_line, rand_hex};
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
    /// A system prompt template for this run alone, in place of the user's or the built-in one.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Skills to switch on, by name.
    #[serde(default)]
    pub skills: Vec<String>,
    /// `false` leaves the project's instruction file (`AGENTS.md`, …) out of the system prompt.
    #[serde(default)]
    pub project_instructions: Option<bool>,
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
    pub base_url: String,
    pub key_env: String,
    pub has_key: bool,
    pub models: Vec<String>,
    pub note: String,
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
    /// A user message, and the skills to switch on with it.
    input: mpsc::UnboundedSender<(String, Vec<SkillInfo>)>,
    cancel: Mutex<CancellationToken>,
    /// Cancelled when the session is closed for good.
    closed: CancellationToken,
    approvals: Arc<Approvals>,
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
    for name in &req.without {
        features = features.without(name).ok_or_else(|| format!("unknown mechanism {name:?}; known: {}", Features::NAMES.join(", ")))?;
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

    /// A harness with this one's keys and defaults that keeps its sessions and lessons elsewhere,
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
                base_url: d.base_url.into(),
                key_env: d.key_env.into(),
                has_key: cfg.has_key(d),
                models: d.models.iter().map(|m| m.to_string()).collect(),
                note: d.note.into(),
            })
            .collect();
        if let Some(c) = &cfg.custom {
            providers.push(ProviderInfo { id: "custom".into(), name: "Custom endpoint".into(), base_url: c.base_url.clone(), key_env: "PRESAGE_API_KEY".into(), has_key: true, models: c.models.clone(), note: String::new() });
        }
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
        if provider != "custom" && !PROVIDERS.iter().any(|d| d.id == provider) {
            return Err(format!("unknown provider {provider:?}"));
        }
        let mut cfg = self.inner.config.lock().unwrap();
        if key.trim().is_empty() {
            cfg.keys.remove(provider);
        } else {
            cfg.keys.insert(provider.to_string(), key.trim().to_string());
        }
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

    fn lessons_path(&self, root: &Path) -> PathBuf {
        self.inner.data_dir.join("projects").join(format!("{:016x}", fnv64(root.as_os_str().as_encoded_bytes()))).join("lessons.jsonl")
    }

    /// Starts a session on a real provider and begins working on the task.
    pub fn start(&self, req: StartRequest) -> Result<SessionMeta, String> {
        if req.task.trim().is_empty() {
            return Err("the task is empty".into());
        }
        let resolved = self.inner.config.lock().unwrap().resolve(req.provider.as_deref(), req.model.as_deref())?;
        let mut client = OpenAiProvider::new(&resolved.base_url, &resolved.key, &resolved.model, resolved.dialect).map_err(|e| e.to_string())?;
        client.keep_retrying = self.inner.auto_retry.clone();
        let provider = Provider::OpenAi(client);
        let effort = req.effort.clone().or_else(|| self.inner.config.lock().unwrap().effort.clone());
        self.launch(req, provider, &resolved.id, &resolved.model, effort)
    }

    /// Starts the scripted walkthrough in a throwaway workspace. No key, no network.
    pub fn demo(&self) -> Result<SessionMeta, String> {
        let root = self.inner.data_dir.join("demo").join(session_id());
        demo::create_workspace(&root).map_err(|e| format!("could not create the demo workspace: {e}"))?;
        let real = crate::util::canonical(&root).map_err(|e| e.to_string())?;
        let provider = Provider::Mock(demo::provider(real, 22));
        // The walkthrough is three replies; the review pass would add a fourth that shows nothing new.
        let req = StartRequest { cwd: root.to_string_lossy().into_owned(), task: demo::TASK.into(), interactive: true, without: vec!["review".into(), "survey".into()], ..Default::default() };
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
        // Read once, here: the system prompt is the cached prefix of every request of the session.
        let (system, picked) = self.compose_prompt(&req, &ws.root, &features, classic)?;
        let skill_names: Vec<String> = picked.into_iter().map(|s| s.name).collect();
        let mode = match (classic, req.without.is_empty()) {
            (true, _) => "classic".to_string(),
            (false, true) => "presage".to_string(),
            (false, false) => format!("presage without {}", req.without.join(", ")),
        };
        let effort = match effort.as_deref() {
            Some(e) => EffortPolicy::parse(e).ok_or_else(|| format!("unknown effort {e:?}; use low, high, max or ratchet"))?,
            None => EffortPolicy::Ratchet(crate::provider::Effort::High),
        };
        let autonomy = match req.autonomy.as_deref() {
            Some("ask") => Autonomy::Ask,
            Some("auto") | None => Autonomy::Auto,
            Some(other) => return Err(format!("unknown autonomy {other:?}; use auto or ask")),
        };

        let id = session_id();
        let dir = self.inner.data_dir.join("sessions").join(&id);
        let lessons = if features.lessons { LessonBook::load(self.lessons_path(&ws.root)) } else { LessonBook::in_memory() };
        let simulated = provider.simulated();
        let cwd = ws.root.to_string_lossy().into_owned();
        let sess = Session::new(ws, dir.clone(), features, lessons, prices_for(model)).map_err(|e| format!("could not create the session directory: {e}"))?;

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
            title: one_line(&req.task, 72),
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
        };
        let (tx, _) = broadcast::channel(4096);
        let (input, mut inbox) = mpsc::unbounded_channel::<(String, Vec<SkillInfo>)>();
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
                let Some((text, skills)) = message else { break };
                let cancel = CancellationToken::new();
                *worker.cancel.lock().unwrap() = cancel.clone();
                worker.set_status(SessionStatus::Running);
                agent.run_with(text, &skills, &cancel).await;
                worker.set_status(SessionStatus::Idle);
            }
            agent.sess.jobs.kill_all();
        });
        live.input.send((req.task, Vec::new())).map_err(|_| "the session ended before it started".to_string())?;
        Ok(meta)
    }

    fn live(&self, id: &str) -> Result<Arc<Live>, String> {
        self.inner.sessions.lock().unwrap().get(id).cloned().ok_or_else(|| format!("session {id} is not running in this process"))
    }

    /// Sends a follow-up message to an idle session, switching on the skills named that it does not
    /// have yet. Its system prompt is already sent, so they go in front of this message instead.
    pub fn send_with_skills(&self, id: &str, text: &str, skills: &[String]) -> Result<(), String> {
        let live = self.live(id)?;
        if text.trim().is_empty() {
            return Err("the message is empty".into());
        }
        let cwd = live.meta.lock().unwrap().cwd.clone();
        let mut added = skills::resolve(&self.inner.data_dir, Some(Path::new(&cwd)), skills)?;
        {
            let mut meta = live.meta.lock().unwrap();
            if meta.status == SessionStatus::Running {
                return Err("the session is still working; stop it first or wait".into());
            }
            // The classic baseline has no skills.
            added.retain(|s| meta.mode != "classic" && !meta.skills.contains(&s.name));
            meta.skills.extend(added.iter().map(|s| s.name.clone()));
            // Mark it running now so a second message cannot slip in before the worker picks this one up.
            meta.status = SessionStatus::Running;
            meta.updated = now_ms();
        }
        live.save_meta();
        live.input.send((text.to_string(), added)).map_err(|_| "the session has ended".to_string())
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
        let req = StartRequest { cwd: cwd.clone(), task: "write a.txt".into(), without: vec!["auto-watch".into(), "review".into()], ..Default::default() };
        let meta = h.start_scripted(req, MockProvider::scripted(turns)).unwrap();
        assert_eq!((meta.mode.as_str(), meta.simulated), ("presage without auto-watch, review", true));
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
        assert_eq!(h.system_prompt(&bare).unwrap(), format!("Mine. Watched tests are re-run first and it is refused if they fail. The first `finish` after a change comes back once, with your changes and a review to carry out.\n\n# User instructions\n\n{user}\n\nUse tabs."));
        assert_eq!(h.system_prompt(&StartRequest { system_prompt: Some("Be terse.\n".into()), ..bare.clone() }).unwrap(), format!("Be terse.\n\n# User instructions\n\n{user}\n\nUse tabs."));
        h.set_system_prompt(None).unwrap();
        assert_eq!(h.system_prompt(&bare).unwrap(), format!("{built_in}\n\n# User instructions\n\n{user}\n\nUse tabs."));

        // The baseline is the classic prompt and nothing else.
        assert_eq!(h.system_prompt(&StartRequest { mode: Some("classic".into()), system_prompt: Some("Be terse.".into()), ..req() }).unwrap(), prompt::SYSTEM_CLASSIC);
        let no_scripts = h.system_prompt(&StartRequest { without: vec!["scripts".into()], ..bare.clone() }).unwrap();
        assert_eq!(no_scripts, format!("{}\n\n# User instructions\n\n{user}\n\nUse tabs.", prompt::SYSTEM_CLASSIC));

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
        let with_message: Vec<(String, Vec<String>)> = events.iter().filter_map(|e| if let EventKind::User { text, skills } = &e.kind { Some((text.clone(), skills.clone())) } else { None }).collect();
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

    #[test]
    fn what_was_stored_before_there_were_skills_still_loads() {
        let meta = SessionMeta { id: "s".into(), title: "t".into(), cwd: "/".into(), provider: "p".into(), model: "m".into(), mode: "presage".into(), effort: "high".into(), simulated: false, created: 1, updated: 2, status: SessionStatus::Idle, outcome: None, verified: None, stats: Stats::default(), skills: vec!["ui".into()] };
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
