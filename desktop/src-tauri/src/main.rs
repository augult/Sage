#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! The desktop shell: one Tauri command per `Harness` operation, the login shell's `PATH`, the
//! headless test hooks, and an orderly end for sessions when the app quits.

use presage_core::events::Event;
use presage_core::harness::{AppState, DirListing, Harness, SessionMeta, SessionStatus, StartRequest};
use presage_core::provider::mock::{MockProvider, MockTurn};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tauri::ipc::Channel;
use tauri::{Emitter, Manager, State};
use tokio::sync::broadcast::error::RecvError;

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
    /// `PRESAGE_UI_SIZE=<width>x<height>`: window size in logical pixels.
    #[serde(skip)]
    size: Option<(f64, f64)>,
    /// `PRESAGE_UI_WALLPAPER=<file>`: the picture behind the glass, instead of the desktop's.
    #[serde(skip)]
    wallpaper: Option<String>,
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
            size,
            wallpaper: var("PRESAGE_UI_WALLPAPER"),
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

/// Where the window's content sits on its monitor, in physical pixels. The UI lays its glass
/// backdrop out over the monitor and shifts it by this, so the backdrop holds still on the screen
/// while the window moves over it, as glass over the desktop would.
#[derive(Clone, Serialize)]
struct Place {
    x: i32,
    y: i32,
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: u32,
    monitor_height: u32,
    scale: f64,
}

fn place_of(window: &tauri::Window) -> Option<Place> {
    let origin = window.inner_position().ok()?;
    let monitor = window.current_monitor().ok()??;
    Some(Place {
        x: origin.x,
        y: origin.y,
        monitor_x: monitor.position().x,
        monitor_y: monitor.position().y,
        monitor_width: monitor.size().width,
        monitor_height: monitor.size().height,
        scale: window.scale_factor().unwrap_or(1.0),
    })
}

/// The window's place now. Later changes arrive as `window-place` events.
#[tauri::command]
async fn window_place(window: tauri::Window) -> Reply<Option<Place>> {
    Ok(place_of(&window))
}

/// Files that may hold the desktop wallpaper, likeliest first. Windows keeps the current one as a
/// JPEG at a fixed place in the profile, and per-size copies beside it; elsewhere there is nothing
/// to find and the UI paints a backdrop of its own. The test hook names the one file to use.
fn wallpaper_candidates(hook: Option<&str>, appdata: Option<&Path>) -> Vec<PathBuf> {
    if let Some(path) = hook {
        return vec![PathBuf::from(path)];
    }
    let Some(appdata) = appdata else { return Vec::new() };
    let themes = appdata.join("Microsoft").join("Windows").join("Themes");
    let mut paths = vec![themes.join("TranscodedWallpaper")];
    let mut cached: Vec<PathBuf> = std::fs::read_dir(themes.join("CachedFiles")).map(|dir| dir.flatten().map(|entry| entry.path()).collect()).unwrap_or_default();
    cached.sort();
    paths.extend(cached);
    paths
}

/// The desktop wallpaper's bytes, for the glass backdrop; empty when there is none to be had. The
/// file is only read here: the webview decodes it, blurs it once and keeps the result.
#[tauri::command]
async fn wallpaper(hooks: State<'_, Hooks>) -> Reply<tauri::ipc::Response> {
    const LIMIT: u64 = 64 * 1024 * 1024;
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    let bytes = wallpaper_candidates(hooks.wallpaper.as_deref(), appdata.as_deref()).into_iter().find_map(|path| {
        let meta = std::fs::metadata(&path).ok()?;
        if !meta.is_file() || meta.len() == 0 || meta.len() > LIMIT {
            return None;
        }
        std::fs::read(&path).ok()
    });
    Ok(tauri::ipc::Response::new(bytes.unwrap_or_default()))
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
    let req = StartRequest { cwd: script.cwd, task: script.task, autonomy: script.autonomy, effort: script.effort, interactive: true, ..Default::default() };
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
        // The glass backdrop is pinned to the screen, so the UI has to hear where the window went.
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. }) {
                if let Some(place) = place_of(window) {
                    let _ = window.emit("window-place", place);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            ui_hooks,
            window_place,
            wallpaper,
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
            list_dir,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallpaper_candidates_follow_the_hook_then_the_windows_profile() {
        let appdata = std::env::temp_dir().join(format!("presage-wallpaper-test-{}", std::process::id()));
        let themes = appdata.join("Microsoft").join("Windows").join("Themes");
        let cached = themes.join("CachedFiles");
        std::fs::create_dir_all(&cached).unwrap();
        std::fs::write(cached.join("CachedImage_1920_1080_POS4.jpg"), b"x").unwrap();

        // The hook names the one file to use, whatever the profile holds.
        assert_eq!(wallpaper_candidates(Some("/tmp/picture.png"), Some(&appdata)), vec![PathBuf::from("/tmp/picture.png")]);
        // No profile (anything but Windows): nothing to read.
        assert!(wallpaper_candidates(None, None).is_empty());
        // The current wallpaper first, then the per-size copies.
        assert_eq!(wallpaper_candidates(None, Some(&appdata)), vec![themes.join("TranscodedWallpaper"), cached.join("CachedImage_1920_1080_POS4.jpg")]);

        std::fs::remove_dir_all(&appdata).unwrap();
    }
}
