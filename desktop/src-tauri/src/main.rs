//! Presage desktop: a thin Tauri shell around `presage_core::harness::Harness`.
//!
//! Every command maps onto one facade call. The UI is the static files in `../ui`; it follows a
//! session through `subscribe`, which hands over the stored events and then forwards live ones.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use presage_core::events::Event;
use presage_core::harness::{AppState, DirListing, Harness, SessionMeta, StartRequest};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tauri::{State, WebviewWindow};
use tokio::sync::broadcast::error::RecvError;

type Cmd<T> = Result<T, String>;

/// The live-event forwarder of each window. A window follows one session at a time, so a new
/// subscription (or a reload, which subscribes again) replaces the previous forwarder instead of
/// adding a second feed to the same view.
#[derive(Default)]
struct Forwarders(Mutex<HashMap<String, JoinHandle<()>>>);

impl Forwarders {
    fn replace(&self, window: &str, task: Option<JoinHandle<()>>) {
        let mut tasks = self.0.lock().unwrap();
        let old = match task {
            Some(task) => tasks.insert(window.to_string(), task),
            None => tasks.remove(window),
        };
        if let Some(old) = old {
            old.abort();
        }
    }
}

/// Test hooks for headless runs, where nobody can click. Read from the environment at startup and
/// handed to the UI once, so that a reload does not replay them:
///
/// - `PRESAGE_UI_AUTODEMO=1` starts the walkthrough session and opens it;
/// - `PRESAGE_UI_VIEW=settings|new|session` opens that view (`session`: the most recent session);
/// - `PRESAGE_UI_OPEN=<session id>` opens a stored session.
#[derive(Clone, Default, Serialize)]
struct DevHooks {
    autodemo: bool,
    view: Option<String>,
    open: Option<String>,
}

impl DevHooks {
    fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        Self { autodemo: var("PRESAGE_UI_AUTODEMO").is_some_and(|v| v != "0"), view: var("PRESAGE_UI_VIEW"), open: var("PRESAGE_UI_OPEN") }
    }
}

#[tauri::command]
async fn dev_hooks(hooks: State<'_, Mutex<Option<DevHooks>>>) -> Cmd<DevHooks> {
    Ok(hooks.lock().unwrap().take().unwrap_or_default())
}

#[tauri::command]
async fn app_state(harness: State<'_, Harness>) -> Cmd<AppState> {
    Ok(harness.state())
}

#[tauri::command]
async fn set_key(harness: State<'_, Harness>, provider: String, key: String) -> Cmd<()> {
    harness.set_key(&provider, &key)
}

#[tauri::command]
async fn set_defaults(harness: State<'_, Harness>, provider: Option<String>, model: Option<String>, effort: Option<String>) -> Cmd<()> {
    harness.set_defaults(provider, model, effort)
}

#[tauri::command]
async fn list_sessions(harness: State<'_, Harness>) -> Cmd<Vec<SessionMeta>> {
    Ok(harness.list())
}

#[tauri::command]
async fn start_session(harness: State<'_, Harness>, mut req: StartRequest) -> Cmd<SessionMeta> {
    // Someone is at the window to answer approvals, whatever the caller sent.
    req.interactive = true;
    harness.start(req)
}

#[tauri::command]
async fn start_demo(harness: State<'_, Harness>) -> Cmd<SessionMeta> {
    harness.demo()
}

#[tauri::command]
async fn send_message(harness: State<'_, Harness>, id: String, text: String) -> Cmd<()> {
    harness.send(&id, &text)
}

#[tauri::command]
async fn stop_session(harness: State<'_, Harness>, id: String) -> Cmd<()> {
    harness.stop(&id)
}

#[tauri::command]
async fn approve(harness: State<'_, Harness>, id: String, approval: u64, allow: bool) -> Cmd<()> {
    harness.approve(&id, approval, allow)
}

#[tauri::command]
async fn session_events(harness: State<'_, Harness>, id: String) -> Cmd<Vec<Event>> {
    harness.events(&id)
}

#[tauri::command]
async fn read_output(harness: State<'_, Harness>, id: String, handle: String) -> Cmd<String> {
    harness.read_output(&id, &handle)
}

#[tauri::command]
async fn list_dir(harness: State<'_, Harness>, path: Option<String>) -> Cmd<DirListing> {
    harness.list_dir(path.as_deref())
}

/// Returns everything stored so far and forwards what follows to `on_event`, until the window is
/// gone, the session's feed closes, or this window subscribes again. Live events can reach the UI
/// before the returned backlog does: the UI queues them and applies each `seq` once, in order.
#[tauri::command]
async fn subscribe(window: WebviewWindow, harness: State<'_, Harness>, forwarders: State<'_, Forwarders>, id: String, on_event: Channel<Event>) -> Cmd<Vec<Event>> {
    // Detach first: the view that asked has moved on, even if this subscription fails.
    forwarders.replace(window.label(), None);
    let (backlog, mut feed) = harness.subscribe(&id)?;
    let mut last = backlog.last().map_or(0, |e| e.seq);
    let harness = harness.inner().clone();
    let task = tauri::async_runtime::spawn(async move {
        loop {
            let batch = match feed.recv().await {
                Ok(event) => vec![event],
                // The view fell further behind than the feed buffers. Stored events are re-read from
                // the session; streamed text that was dropped is superseded by the turn's `model` event.
                Err(RecvError::Lagged(_)) => harness.events(&id).unwrap_or_default(),
                Err(RecvError::Closed) => return,
            };
            for event in batch {
                if event.seq <= last {
                    continue;
                }
                last = event.seq;
                if on_event.send(event).is_err() {
                    return;
                }
            }
        }
    });
    forwarders.replace(window.label(), Some(task));
    Ok(backlog)
}

/// Stops this window's forwarder. The UI calls it when it starts up (a reload leaves the old page's
/// forwarder behind) and when it replaces a live session with a stored one.
#[tauri::command]
async fn unsubscribe(window: WebviewWindow, forwarders: State<'_, Forwarders>) -> Cmd<()> {
    forwarders.replace(window.label(), None);
    Ok(())
}

/// What the user's login shell puts in `PATH`, or `None` if that cannot be found out within 3 s.
#[cfg(unix)]
fn login_shell_path() -> Option<String> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const MARK: &str = "__PRESAGE_PATH__";
    let shell = std::env::var_os("SHELL").filter(|s| !s.is_empty())?;
    // The output goes to a file rather than a pipe: something an rc file leaves running can hold a
    // pipe open long after the shell has exited. The markers cut out whatever rc files print.
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let out_path = std::env::temp_dir().join(format!("presage-path-{}-{nanos}", std::process::id()));
    let out = std::fs::OpenOptions::new().write(true).create_new(true).open(&out_path).ok()?;
    let script = format!(r#"printf '{MARK}%s{MARK}' "$PATH""#);
    let finished = Command::new(shell).args(["-ilc", &script]).stdin(Stdio::null()).stdout(out).stderr(Stdio::null()).spawn().ok().is_some_and(|mut child| {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(15)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
            }
        }
    });
    let printed = std::fs::read(&out_path);
    let _ = std::fs::remove_file(&out_path);
    if !finished {
        return None;
    }
    let text = String::from_utf8_lossy(&printed.ok()?).into_owned();
    let end = text.rfind(MARK)?;
    let start = text[..end].rfind(MARK)? + MARK.len();
    Some(text[start..end].to_string())
}

/// Apps started from Finder or a desktop launcher inherit a minimal `PATH`, so the agent's shell
/// would miss the user's tools. Adopts the login shell's `PATH` instead; failures change nothing.
/// Must run before any other thread exists, because that is the only time `set_var` is sound.
#[cfg(unix)]
fn adopt_login_shell_path() {
    let Some(path) = login_shell_path().filter(|p| !p.is_empty()) else { return };
    if std::env::var_os("PATH").is_none_or(|current| current != path.as_str()) {
        // SAFETY: first statement of `main`; neither Tauri nor tokio has started a thread yet.
        unsafe { std::env::set_var("PATH", path) };
    }
}

fn main() {
    #[cfg(unix)]
    adopt_login_shell_path();

    tauri::Builder::default()
        .manage(Harness::new())
        .manage(Forwarders::default())
        .manage(Mutex::new(Some(DevHooks::from_env())))
        .invoke_handler(tauri::generate_handler![
            dev_hooks,
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
            subscribe,
            unsubscribe,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Presage");
}
