#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! The desktop shell: one Tauri command per `Harness` operation, and nothing else.

use presage_core::events::Event;
use presage_core::harness::{AppState, DirListing, Harness, SessionMeta, StartRequest};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tokio::sync::broadcast::error::RecvError;

type Reply<T> = Result<T, String>;

/// Headless test hooks. Every `PRESAGE_UI_*` variable is read here and nowhere else.
#[derive(Clone, Default, Serialize)]
struct Hooks {
    /// `PRESAGE_UI_AUTODEMO=1`: start the walkthrough and open it.
    autodemo: bool,
    /// `PRESAGE_UI_VIEW=settings|new|session`.
    view: Option<String>,
    /// `PRESAGE_UI_OPEN=<session id>`.
    open: Option<String>,
    /// `PRESAGE_UI_SCRIPT`: UI actions to play once the first view is up (grammar in `ui/js/hooks.js`).
    script: Option<String>,
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
        Self { autodemo: var("PRESAGE_UI_AUTODEMO").as_deref() == Some("1"), view: var("PRESAGE_UI_VIEW"), open: var("PRESAGE_UI_OPEN"), script: var("PRESAGE_UI_SCRIPT"), size }
    }
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
    if std::io::stdin().is_terminal() {
        return;
    }
    let Some(shell) = std::env::var_os("SHELL").filter(|s| !s.is_empty()) else { return };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
    let printed = runtime.block_on(async {
        // Start-up files may print before the command runs; the markers isolate the value.
        let script = format!("printf '{MARK}%s{MARK}' \"$PATH\"");
        let run = tokio::process::Command::new(shell).args(["-ilc", &script]).stdin(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true).output();
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
fn ui_hooks(hooks: State<'_, Hooks>) -> Hooks {
    hooks.inner().clone()
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

#[tauri::command]
async fn send_message(harness: State<'_, Harness>, id: String, text: String) -> Reply<()> {
    harness.send(&id, &text)
}

#[tauri::command]
async fn stop_session(harness: State<'_, Harness>, id: String) -> Reply<()> {
    harness.stop(&id)
}

#[tauri::command]
async fn approve(harness: State<'_, Harness>, id: String, approval: u64, allow: bool) -> Reply<()> {
    harness.approve(&id, approval, allow)
}

#[tauri::command]
async fn session_events(harness: State<'_, Harness>, id: String) -> Reply<Vec<Event>> {
    harness.events(&id)
}

#[tauri::command]
async fn read_output(harness: State<'_, Harness>, id: String, handle: String) -> Reply<String> {
    harness.read_output(&id, &handle)
}

#[tauri::command]
async fn list_dir(harness: State<'_, Harness>, path: Option<String>) -> Reply<DirListing> {
    harness.list_dir(path.as_deref())
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

fn main() {
    #[cfg(unix)]
    adopt_login_shell_path();

    let hooks = Hooks::from_env();
    let size = hooks.size;
    tauri::Builder::default()
        .manage(Harness::new())
        .manage(hooks)
        .setup(move |app| {
            if let (Some((width, height)), Some(window)) = (size, app.get_webview_window("main")) {
                window.set_size(tauri::LogicalSize::new(width, height))?;
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
            send_message,
            stop_session,
            approve,
            session_events,
            read_output,
            list_dir,
            subscribe
        ])
        .run(tauri::generate_context!())
        .expect("error while running Presage");
}
