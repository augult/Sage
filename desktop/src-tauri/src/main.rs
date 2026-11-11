#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! The desktop shell: one Tauri command per `Harness` operation, the login shell's `PATH`, the
//! headless test hooks, and an orderly end for sessions when the app quits.

use presage_core::events::Event;
use presage_core::harness::{AppState, Harness, Instructions, SessionMeta, SessionStatus, StartRequest};
use presage_core::provider::mock::{MockProvider, MockTurn};
use presage_core::skills::SkillInfo;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tokio::sync::broadcast::error::RecvError;

mod git;

type Reply<T> = Result<T, String>;

/// Headless test hooks. Every `PRESAGE_UI_*` variable is read here and nowhere else.
#[derive(Clone, Serialize)]
struct Hooks {
    /// `PRESAGE_UI_AUTODEMO=1`: start the walkthrough and open it.
    autodemo: bool,
    /// `PRESAGE_UI_VIEW=settings|new|session`.
    view: Option<String>,
    /// `PRESAGE_UI_OPEN=<session id>`.
    open: Option<String>,
    /// `PRESAGE_UI_SCRIPTED=<file>`: start a session played by a scripted stand-in and open it
    /// (see `start_scripted`). The only way to reach a live approval request without a provider.
    scripted: Option<String>,
    /// `PRESAGE_UI_SCRIPT`: UI actions to play once the first view is up (grammar in `ui/js/hooks.js`).
    script: Option<String>,
    /// `PRESAGE_UI_THEME=<background>`: the painting behind the glass for this launch, by its id
    /// (`ui/js/themes.js`), whatever the user has chosen.
    theme: Option<String>,
    /// `PRESAGE_UI_SCHEME=light|dark`: the appearance for this launch.
    scheme: Option<String>,
    /// `PRESAGE_UI_SIZE=<width>x<height>`: window size in logical pixels.
    #[serde(skip)]
    size: Option<(f64, f64)>,
}

impl Hooks {
    fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let size = var("PRESAGE_UI_SIZE").and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
        });
        Self {
            autodemo: var("PRESAGE_UI_AUTODEMO").as_deref() == Some("1"),
            view: var("PRESAGE_UI_VIEW"),
            open: var("PRESAGE_UI_OPEN"),
            scripted: var("PRESAGE_UI_SCRIPTED"),
            script: var("PRESAGE_UI_SCRIPT"),
            theme: var("PRESAGE_UI_THEME"),
            scheme: var("PRESAGE_UI_SCHEME"),
            size,
        }
    }
}

/// What `PRESAGE_UI_SCRIPTED` points at: a task and the replies the stand-in gives, in order.
#[derive(Deserialize)]
struct Scripted {
    cwd: String,
    task: String,
    #[serde(default)]
    autonomy: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    /// Skills the session starts with, by name.
    #[serde(default)]
    skills: Vec<String>,
    /// Milliseconds between streamed chunks of a reply.
    #[serde(default)]
    delay_ms: u64,
    turns: Vec<ScriptedTurn>,
}

#[derive(Deserialize)]
struct ScriptedTurn {
    #[serde(default)]
    reasoning: String,
    #[serde(default)]
    content: String,
    /// `[tool, arguments]` pairs.
    #[serde(default)]
    calls: Vec<(String, Value)>,
}

/// GUI launchers hand an app a minimal `PATH`; the commands an agent runs need the one the user's
/// shell has. Asks the login shell for it and adopts it, giving up after three seconds.
#[cfg(unix)]
fn adopt_login_shell_path() {
    use std::io::IsTerminal;
    use std::process::Stdio;
    use std::time::Duration;

    const MARK: &str = "__PRESAGE_PATH__";
    // Started from a terminal, the environment already is that shell's, and an interactive child
    // shell would compete with it for the terminal.
    if std::io::stdin().is_terminal() || std::io::stdout().is_terminal() || std::io::stderr().is_terminal() {
        return;
    }
    let Some(shell) = std::env::var_os("SHELL").filter(|s| !s.is_empty()) else { return };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
    let printed = runtime.block_on(async {
        // Start-up files may print before the command runs; the markers isolate the value.
        let script = format!("printf '{MARK}%s{MARK}' \"$PATH\"");
        // Its own process group: an interactive shell that finds itself in the background of a
        // terminal stops its whole group, which must not include this app.
        let run = tokio::process::Command::new(shell).args(["-ilc", &script]).stdin(Stdio::null()).stderr(Stdio::null()).process_group(0).kill_on_drop(true).output();
        tokio::time::timeout(Duration::from_secs(3), run).await
    });
    drop(runtime);
    let Ok(Ok(output)) = printed else { return };
    let text = String::from_utf8_lossy(&output.stdout);
    if let Some(path) = text.split(MARK).nth(1).filter(|p| !p.trim().is_empty()) {
        // SAFETY: this is the first thing `main` does. The runtime above was single-threaded and is
        // gone, so no other thread can be reading the environment.
        unsafe { std::env::set_var("PATH", path) };
    }
}

#[tauri::command]
async fn ui_hooks(hooks: State<'_, Hooks>) -> Reply<Hooks> {
    Ok(hooks.inner().clone())
}

/// Test hook: starts the session `PRESAGE_UI_SCRIPTED` describes. The path comes from the
/// environment only, never from the webview, and no provider is contacted.
#[tauri::command]
async fn start_scripted(harness: State<'_, Harness>, hooks: State<'_, Hooks>) -> Reply<SessionMeta> {
    let path = hooks.scripted.as_deref().ok_or("PRESAGE_UI_SCRIPTED is not set")?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let script: Scripted = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let turns = script.turns.into_iter().map(|t| MockTurn { reasoning: t.reasoning, content: t.content, calls: t.calls, finish: None }).collect();
    let mut provider = MockProvider::scripted(turns);
    provider.chunk_delay_ms = script.delay_ms;
    let req = StartRequest { cwd: script.cwd, task: script.task, autonomy: script.autonomy, effort: script.effort, skills: script.skills, interactive: true, ..Default::default() };
    harness.start_scripted(req, provider)
}

#[tauri::command]
async fn app_state(harness: State<'_, Harness>) -> Reply<AppState> {
    Ok(harness.state())
}

/// Stores a key, or removes it when `key` is empty. Keys never travel the other way.
#[tauri::command]
async fn set_key(harness: State<'_, Harness>, provider: String, key: String) -> Reply<()> {
    harness.set_key(&provider, &key)
}

#[tauri::command]
async fn set_defaults(harness: State<'_, Harness>, provider: Option<String>, model: Option<String>, effort: Option<String>) -> Reply<()> {
    harness.set_defaults(provider, model, effort)
}

#[tauri::command]
async fn list_sessions(harness: State<'_, Harness>) -> Reply<Vec<SessionMeta>> {
    Ok(harness.list())
}

#[tauri::command]
async fn start_session(harness: State<'_, Harness>, mut req: StartRequest) -> Reply<SessionMeta> {
    // Someone is at the window to answer approval requests.
    req.interactive = true;
    harness.start(req)
}

#[tauri::command]
async fn start_demo(harness: State<'_, Harness>) -> Reply<SessionMeta> {
    harness.demo()
}

/// `skills` are switched on with this message.
#[tauri::command]
async fn send_message(harness: State<'_, Harness>, id: String, text: String, skills: Option<Vec<String>>) -> Reply<()> {
    harness.send_with_skills(&id, &text, &skills.unwrap_or_default())
}

#[tauri::command]
async fn stop_session(harness: State<'_, Harness>, id: String) -> Reply<()> {
    harness.stop(&id)
}

#[tauri::command]
async fn approve(harness: State<'_, Harness>, id: String, approval: u64, allow: bool) -> Reply<()> {
    harness.approve(&id, approval, allow)
}

/// Stored events of any session, running or past. A past session's log is handed over line by
/// line as it was written: `Harness::events` drops every line this build cannot type, and an event
/// from a newer version of the engine should still reach the timeline.
#[tauri::command]
async fn session_events(harness: State<'_, Harness>, id: String) -> Reply<Vec<Value>> {
    if harness.meta(&id).is_err() && !id.contains(['/', '\\', '.']) {
        let log = Path::new(&harness.state().data_dir).join("sessions").join(&id).join("events.jsonl");
        if let Ok(text) = std::fs::read_to_string(log) {
            let event = |line: &str| {
                let parsed = serde_json::from_str::<Value>(line).ok().filter(Value::is_object);
                // A line that is not an event (a torn write, say) is shown as such rather than skipped.
                parsed.unwrap_or_else(|| json!({ "type": "unreadable", "text": line.chars().take(240).collect::<String>() }))
            };
            return Ok(text.lines().filter(|line| !line.trim().is_empty()).map(event).collect());
        }
    }
    harness.events(&id)?.iter().map(|event| serde_json::to_value(event).map_err(|e| e.to_string())).collect()
}

#[tauri::command]
async fn read_output(harness: State<'_, Harness>, id: String, handle: String) -> Reply<String> {
    harness.read_output(&id, &handle)
}

/// Asks for a folder with the system's own dialog (Explorer, Finder, the desktop's file chooser),
/// opened at `start` when that is a folder. Nothing comes back when the dialog is dismissed.
#[tauri::command]
async fn pick_folder(app: tauri::AppHandle, start: Option<String>) -> Reply<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let mut dialog = app.dialog().file().set_title("Choose a folder");
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    if let Some(dir) = start.map(PathBuf::from).filter(|dir| dir.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    let (chosen, answer) = tokio::sync::oneshot::channel();
    dialog.pick_folder(move |folder| {
        let _ = chosen.send(folder);
    });
    let folder = answer.await.map_err(|_| "the folder dialog closed without an answer".to_string())?;
    Ok(folder.and_then(|folder| folder.into_path().ok()).map(|path| presage_core::util::plain_path(&path.to_string_lossy())))
}

/// Built-in and user skills, plus the project's own when `cwd` is given.
#[tauri::command]
async fn list_skills(harness: State<'_, Harness>, cwd: Option<String>) -> Reply<Vec<SkillInfo>> {
    Ok(harness.skills(cwd.as_deref()))
}

#[tauri::command]
async fn skill_file(harness: State<'_, Harness>, name: String, cwd: Option<String>) -> Reply<String> {
    harness.skill_file(&name, cwd.as_deref())
}

#[tauri::command]
async fn save_skill(harness: State<'_, Harness>, name: String, content: String) -> Reply<SkillInfo> {
    harness.save_skill(&name, &content)
}

#[tauri::command]
async fn delete_skill(harness: State<'_, Harness>, name: String) -> Reply<()> {
    harness.delete_skill(&name)
}

#[tauri::command]
async fn instructions(harness: State<'_, Harness>) -> Reply<Instructions> {
    Ok(harness.instructions())
}

/// `None` or nothing goes back to the built-in template.
#[tauri::command]
async fn set_system_prompt(harness: State<'_, Harness>, template: Option<String>) -> Reply<()> {
    harness.set_system_prompt(template.as_deref())
}

#[tauri::command]
async fn set_global_instructions(harness: State<'_, Harness>, text: String) -> Reply<()> {
    harness.set_global_instructions(&text)
}

/// The folder a session works in, whether it is running or from an earlier launch.
fn session_cwd(harness: &Harness, id: &str) -> Reply<PathBuf> {
    harness.list().into_iter().find(|meta| meta.id == id).map(|meta| PathBuf::from(meta.cwd)).ok_or_else(|| format!("no session {id}"))
}

/// Where the session's workspace stands with git: branch, remote, what is uncommitted or unpushed.
#[tauri::command]
async fn git_state(harness: State<'_, Harness>, id: String) -> Reply<git::GitState> {
    Ok(git::state(&session_cwd(&harness, &id)?).await)
}

/// Commits what is uncommitted in the session's workspace under `message` and pushes its branch.
#[tauri::command]
async fn git_push(harness: State<'_, Harness>, id: String, message: String) -> Reply<git::Pushed> {
    git::push(&session_cwd(&harness, &id)?, &message).await
}

/// Returns everything stored so far and forwards what follows to `on_event`, with nothing lost or
/// repeated in between. Forwarding ends when the webview or the session goes away.
#[tauri::command]
async fn subscribe(harness: State<'_, Harness>, id: String, on_event: Channel<Event>) -> Reply<Vec<Event>> {
    let (backlog, mut live) = harness.subscribe(&id)?;
    tokio::spawn(async move {
        loop {
            match live.recv().await {
                Ok(event) => {
                    if on_event.send(event).is_err() {
                        break;
                    }
                }
                // Events were dropped because the webview fell behind. The UI sees the jump in `seq`
                // and reloads the stored events, so keep forwarding.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });
    Ok(backlog)
}

/// Ends every session this process runs. Without it, a command an agent started (a test run, a
/// server it left in the background) would go on running after the app has gone.
fn close_sessions(harness: &Harness) {
    let live: Vec<String> = harness.list().into_iter().filter(|meta| meta.status != SessionStatus::Closed).map(|meta| meta.id).collect();
    for id in &live {
        let _ = harness.close(id);
    }
    // `close` only signals the session tasks, and they do the killing: leave them a moment for it.
    if !live.is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

fn main() {
    #[cfg(unix)]
    adopt_login_shell_path();

    let hooks = Hooks::from_env();
    let size = hooks.size;
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Harness::new())
        .manage(hooks)
        .setup(move |app| {
            if let (Some((width, height)), Some(window)) = (size, app.get_webview_window("main")) {
                window.set_size(tauri::LogicalSize::new(width, height))?;
            }
            // Being terminated (a logout, `kill`, Ctrl-C) leaves through the same door as quitting.
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let (Ok(mut terminate), Ok(mut interrupt)) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt())) else { return };
                    tokio::select! {
                        _ = terminate.recv() => {}
                        _ = interrupt.recv() => {}
                    }
                    handle.exit(0);
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ui_hooks,
            app_state,
            set_key,
            set_defaults,
            list_sessions,
            start_session,
            start_demo,
            start_scripted,
            send_message,
            stop_session,
            approve,
            session_events,
            read_output,
            pick_folder,
            list_skills,
            skill_file,
            save_skill,
            delete_skill,
            instructions,
            set_system_prompt,
            set_global_instructions,
            git_state,
            git_push,
            subscribe
        ])
        .build(tauri::generate_context!())
        .expect("error while starting Presage");
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            close_sessions(handle.state::<Harness>().inner());
        }
    });
}
