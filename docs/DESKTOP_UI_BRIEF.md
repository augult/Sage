# Desktop UI brief

The shell and front end are built; what has and has not been verified is in `docs/HANDOFF.md`. This
brief remains the specification. Everything the UI needs is on `presage_core::harness::Harness`.

## Read first

- `crates/presage-core/src/harness.rs`: `Harness::new()`, `state()`, `set_key`, `set_defaults`,
  `start(StartRequest)`, `demo()`, `send`, `stop`, `approve`, `subscribe(id)` (backlog plus a
  `broadcast::Receiver<Event>`), `events(id)`, `meta(id)`, `list()`, `read_output(id, handle)`,
  `list_dir(path)`; types `SessionMeta`, `SessionStatus`, `StartRequest`, `AppState`, `DirListing`.
- `crates/presage-core/src/events.rs`: `Event { seq, ts, type, …fields }` (flattened; `type` is
  snake_case), `Stats`, `FinishStatus`, `CacheVerdict`. `delta` events are transient streaming text;
  a `retry` event voids what was streamed for that turn.
- `crates/presage-cli/src/main.rs`: the terminal renderer, a reference for presenting each event.
- `presage demo` writes a complete session to `$PRESAGE_HOME/sessions/<id>/events.jsonl`.

`Harness::start` and `demo` call `tokio::spawn`: invoke them from async Tauri commands. `Harness` is
`Clone`; keep one in managed state.

## Shell (`desktop/src-tauri`)

Scaffold exists (Tauri 2.12, `withGlobalTauri: true`, `frontendDist: ../ui`, CSP, capability, icons).
Dependencies are limited to what `Cargo.toml` lists.

Commands, each `Result<T, String>`: `app_state`, `set_key(provider, key)`,
`set_defaults(provider?, model?, effort?)`, `list_sessions`, `start_session(req)` (force
`interactive = true`), `start_demo`, `send_message(id, text)`, `stop_session(id)`,
`approve(id, approval, allow)`, `session_events(id)`, `read_output(id, handle)`, `list_dir(path?)`,
and `subscribe(id, on_event: tauri::ipc::Channel<Event>) -> Vec<Event>`, which returns the backlog
and forwards live events until the channel or broadcast closes (keep going on `Lagged`). The UI
should ignore events whose `seq` it has already applied.

GUI apps start with a minimal `PATH`. On unix, first thing in `main`, resolve the login shell's
`PATH` (`$SHELL -ilc 'printf %s "$PATH"'`, 3 s timeout) and adopt it.

Headless test hooks via environment: `PRESAGE_UI_AUTODEMO=1` (start and open the demo),
`PRESAGE_UI_VIEW=settings|new|session`, `PRESAGE_UI_OPEN=<session id>`. Added while building it:
`PRESAGE_UI_SIZE=<width>x<height>`, `PRESAGE_UI_SCRIPTED=<file>` (start a session played by a
scripted stand-in and open it), `PRESAGE_UI_SCRIPT` (UI actions to play once the first view is up;
grammar in `desktop/ui/js/hooks.js`).

## Front end (`desktop/ui`)

Static files, no bundler, no framework, no external resources. `window.__TAURI__.core.invoke` and
`new window.__TAURI__.core.Channel()`. Never put event, model or tool text through `innerHTML`.

Look: a calm, dense developer tool. Dark by default, system UI font, monospace for observations,
8px grid, thin borders. Palette: background `#0f1218`, text `#e6e8ee`, muted `#8b93a7`, prediction
violet `#7c5cff`, held/ok teal `#2dd4bf`, **surprise amber `#f5a524`**, error red `#f31260`. The icon
is two overlapping circles, prediction and reality.

Three columns:

1. **Sidebar**: app name, New session, session list (title, model, status glyph, relative time),
   Settings.
2. **Main**:
   - *New session*: workspace field with an in-app folder picker on `list_dir`; provider and model
     from `app_state` (providers without a key are shown as "needs key"); effort (ratchet, low, high,
     max); mode Presage / Classic; autonomy Auto / Ask; task textarea; Start (Ctrl/Cmd+Enter). With no
     key configured, lead with "Play the walkthrough" (`start_demo`) and "Add an API key".
   - *Session*: header (title, cwd, model, effort, mode, SIMULATED badge, Stop). Timeline: the task,
     the environment snapshot (collapsed), then a card per turn: number, effort, duration, tokens with
     cached share, cache verdict; reasoning (streams live, collapses after the turn); the script as
     step rows on a rail, each with status glyph, tool, summary, an "expect …" chip that turns held or
     violated, duration, and the observation (expanded for surprise, error, denied, running). Skipped
     steps are dimmed with the reason. Watch changes, harness notes, lessons, effort changes, trims
     and retries appear as slim inline chips. Approval requests are a prominent card with Allow /
     Deny. A result card closes the run: status, verified badge, evidence, summary. Idle sessions get
     a follow-up box; closed ones are read-only replays from `session_events`. `out:N` in an
     observation opens the full output via `read_output`.
   - *Settings*: per provider a password field with Save / Remove (`set_key`), the equivalent
     environment variable, and defaults (`set_defaults`). Never show stored keys.
3. **Inspector**: Economy from `stats` events (turns, steps, surprises, predictions held of made,
   prompt tokens with cached share, output with thinking share, cost "at list price" or "fabricated"
   when simulated, context size, model versus tool time, output kept out of context, repairs,
   retries, cache misses); Watches with current state; Notes and lessons.

## Build and verify

```sh
cargo build -p presage-desktop --target-dir target/desktop

PRESAGE_HOME=/tmp/presage-ui-shots PRESAGE_UI_AUTODEMO=1 \
WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 \
WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 LIBGL_ALWAYS_SOFTWARE=1 \
timeout 60 xvfb-run -a -s "-screen 0 1320x860x24" bash -c \
  'target/desktop/debug/presage-desktop & APP=$!; sleep 12; import -window root /tmp/shot.png; kill $APP; wait $APP 2>/dev/null; true'
```

Look at every screenshot. The demo session must show the surprise and the skipped `finish` clearly.
The walkthrough: reply 1 reads and runs the check (it becomes watch `tests`), reply 2 edits and
predicts a pass (a surprise; `finish` is skipped), reply 3 edits and finishes (verified by watch
`tests`).
