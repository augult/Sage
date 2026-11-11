# Desktop UI brief

The shell and front end are built; what has and has not been verified is in `docs/HANDOFF.md`. This
brief remains the specification. Everything the UI needs is on `presage_core::harness::Harness`.

## Read first

- `crates/presage-core/src/harness.rs`: `Harness::new()`, `state()`, `set_key`, `set_defaults`,
  `start(StartRequest)`, `demo()`, `stop`, `approve`, `subscribe(id)` (backlog plus a
  `broadcast::Receiver<Event>`), `events(id)`, `meta(id)`, `list()`, `read_output(id, handle)`,
  `list_dir(path)`; for skills and instructions `skills(cwd)`, `skill_file`, `save_skill`,
  `delete_skill`, `send_with_skills`, `instructions()`, `set_system_prompt`,
  `set_global_instructions`; types `SessionMeta`, `SessionStatus`, `StartRequest`, `AppState`,
  `DirListing`, `Instructions`, and `skills::SkillInfo`.
- `crates/presage-core/src/events.rs`: `Event { seq, ts, type, …fields }` (flattened; `type` is
  snake_case), `Stats`, `FinishStatus`, `CacheVerdict`. `delta` events are transient streaming text;
  a `retry` event voids what was streamed for that turn.
- `crates/presage-cli/src/main.rs`: the terminal renderer, a reference for presenting each event.
- `presage demo` writes a complete session to `$PRESAGE_HOME/sessions/<id>/events.jsonl`.

`Harness::start` and `demo` call `tokio::spawn`: invoke them from async Tauri commands. `Harness` is
`Clone`; keep one in managed state.

## Shell (`desktop/src-tauri`)

Scaffold exists (Tauri 2.12, `withGlobalTauri: true`, `frontendDist: ../ui`, CSP, capability, icons).
Dependencies are limited to what `Cargo.toml` lists. The window is undecorated (`decorations: false`,
`shadow: false`, so Windows 10 draws no white frame line): the UI draws the title bar, and the
capability lets the window drag, minimise, maximise and close itself.

Commands, each `Result<T, String>`: `app_state`, `set_key(provider, key)`,
`set_defaults(provider?, model?, effort?)`, `list_sessions`, `start_session(req)` (force
`interactive = true`; `req.skills` names the skills to start with), `start_demo`,
`send_message(id, text, skills?)` (`send_with_skills`: the skills named are switched on with the
message), `stop_session(id)`,
`approve(id, approval, allow)`, `session_events(id)`, `read_output(id, handle)`, `list_dir(path?)`,
and `subscribe(id, on_event: tauri::ipc::Channel<Event>) -> Vec<Event>`, which returns the backlog
and forwards live events until the channel or broadcast closes (keep going on `Lagged`). The UI
should ignore events whose `seq` it has already applied.

Skills and instructions: `list_skills(cwd?)` (built-in and user skills, plus the project's own when
a folder is given), `skill_file(name, cwd?)` (the whole file, front matter included),
`save_skill(name, content)`, `delete_skill(name)` (a user skill; one that replaced a built-in gives
the built-in back), `instructions()` (`{ system, system_default, system_custom, global }`),
`set_system_prompt(template?)` (nothing resets to the built-in) and `set_global_instructions(text)`
(empty deletes the file).

For the glass: `window_place` (where the window's content sits on its monitor, in physical pixels;
later changes arrive as `window-place` events) and `wallpaper` (the bytes of the desktop wallpaper,
empty when there is none to read: Windows keeps it at `%APPDATA%\Microsoft\Windows\Themes`).

GUI apps start with a minimal `PATH`. On unix, first thing in `main`, resolve the login shell's
`PATH` (`$SHELL -ilc 'printf %s "$PATH"'`, 3 s timeout) and adopt it.

Headless test hooks via environment: `PRESAGE_UI_AUTODEMO=1` (start and open the demo),
`PRESAGE_UI_VIEW=settings|new|session`, `PRESAGE_UI_OPEN=<session id>`. Added while building it:
`PRESAGE_UI_SIZE=<width>x<height>`, `PRESAGE_UI_SCRIPTED=<file>` (start a session played by a
scripted stand-in and open it), `PRESAGE_UI_SCRIPT` (UI actions to play once the first view is up;
grammar in `desktop/ui/js/hooks.js`), `PRESAGE_UI_WALLPAPER=<file>` (the picture behind the glass).
A scripted session's file may name `skills` to start with.
Pickers answer to scripts as `#pick-model`, `#pick-skills`, `#pick-effort`, `#pick-mode`,
`#pick-autonomy`, and their options as `#pick-mode-menu [data-value="classic"]`. The Skills button
is disabled until its list has loaded: click `#pick-skills:not(:disabled)`. In Settings:
`#settings-skills`, `#settings-system`, `#settings-instructions`, a skill's row as
`[data-skill="debug"]`, `#new-skill`, the text areas `#skill-file`, `#system-prompt`,
`#global-instructions`, and each section's `.editor-save`.

## Front end (`desktop/ui`)

Static files, no bundler, no framework, no external resources. `window.__TAURI__.core.invoke` and
`new window.__TAURI__.core.Channel()`. Never put event, model or tool text through `innerHTML`.

### Look

Neutral dark glass: dense, quiet, and the same grey whatever is behind it.

- **Type.** Inter, bundled (`ui/fonts/`, SIL OFL), at four sizes (11, 12, 13, 14 px) and three
  weights (400, 500, 600). The system monospace for paths, commands and observations only.
- **Shape.** 8px grid. Radius grows with the thing: 5 tags, 7 controls and rows, 10 cards,
  14 composers and dialogs. Nothing is a pill.
- **Colour is kept for meaning; the chrome has none.** Azure `#6ea8ff` is the model's side (a
  prediction, a run in progress), teal `#2dd4bf` held or ok, **amber `#f5a524` surprise**, red
  `#ef4b55` error. Every state also has a word or a glyph. The primary button is light on dark
  with no hue, one per view. No gradients, glows or accent borders.
- **Boxes are rare.** Turns are separated by a rule. Only the task, an approval request and the
  result are boxed, so the result is what the eye lands on.
- **Motion.** A view or dialog arriving fades in over 140 ms; a disclosure chevron turns. Nothing
  else moves, and `prefers-reduced-motion` removes those too.
- **No explanatory text.** A label, a value, a state ("Read-only", "No watches", "Unsaved") or an
  error is shown; a sentence that explains how the product works is not, in a note, a caption, a
  placeholder or a tooltip. Placeholders are a word or two ("Folder", "Task"), empty states two or
  three. Where a control is unclear without a sentence, the control is changed. What the engine
  itself reports about a provider (a gateway that logs prompts) is data: it is the `title` of the
  provider's name in Settings, not a line of its own.
- **Title bar.** The top 44px of each pane is the title bar (`data-tauri-drag-region`), with the
  caption buttons where Windows puts them.

### Glass (`ui/js/glass.js`, `ui/styles/glass.css`)

No `backdrop-filter` and no system acrylic: both blur again on every frame, which is what makes
acrylic windows lag on Windows 10. The app makes the material once, at start-up. It takes the
desktop wallpaper (or paints a neutral field when there is none to read), blurs it at 128px,
presses its brightness into a dark band, and draws the result to one canvas behind the panes. The
canvas is laid out over the monitor and shifted by the window's position, so it holds still on the
screen while the window moves over it; only its transform ever changes. The panes are translucent
fills (sidebar thinnest, main thickest), with no grain over them. It shows the wallpaper, not other
windows. `prefers-reduced-transparency` swaps in solid panes.

Contrast is part of the material. `--text`, `--soft` and `--muted` keep 4.5:1 on every pane for any
wallpaper (computed over a 5,832-colour grid; the lowest is 4.8:1, muted on the bare sidebar).
`--faint` holds it only on the main pane and in wells, so it is used for placeholders and key hints
there and for marks elsewhere. On a hovered or selected row, secondary text steps up to `--soft`.
Change a tint, the brightness band in `glass.js` or an ink colour, and these have to be recomputed.

### Panes

1. **Sidebar**: app mark and name (the strip drags the window), New session, the sessions under a
   heading per day (title, model, status glyph, relative time), Settings and the version.
2. **Main**:
   - *New session*: one composer. Along its top the workspace field and Browse (an in-app folder
     picker on `list_dir`); then the task; then pickers for model, skills, effort (ratchet, low,
     high, max), mode (Presage / Classic) and autonomy (Auto / Ask), and Start (Ctrl/Cmd+Enter). The
     model menu lists models under their provider from `app_state`; a provider without a key is
     shown as "needs key" with its models disabled. The Skills button sits next to the model and
     uses the same menu: it lists the skills of the chosen folder (`list_skills`) by title, each
     with its description as the `title` attribute and a check mark when on; several can be on,
     the menu stays open while toggling, and arrows, Home/End, Enter/Space and Escape work as in
     the other menus. The button reads "Skills" with nothing on, then "Skills" and the number on
     (the titles are its `title` attribute), so the bar keeps to one row. A
     new session starts with no skills, whatever the last one had; Classic takes none, so the
     button is disabled there. With no key configured, the view leads with "Play the walkthrough"
     (`start_demo`) and "Add an API key", and Start is not the primary button.
   - *Session*: header (title, cwd, model, active skills, effort, mode, SIMULATED badge, Stop). Timeline: the task,
     the environment snapshot (collapsed), then a section per turn: number, effort, duration, tokens with
     cached share, cache verdict; reasoning (streams live, collapses after the turn); the script as
     step rows on a rail, each with status glyph, tool, summary, an "expect …" chip that turns held or
     violated, duration, and the observation (expanded for surprise, error, denied, running). Skipped
     steps are dimmed with the reason. Watch changes, harness notes, lessons, effort changes, trims
     and retries appear as slim inline chips. Approval requests are a prominent card with Allow /
     Deny. A result card closes the run: status, verified badge, evidence, summary. Idle sessions get
     a follow-up box with the same Skills button: the session's skills are checked and locked, and
     any others picked are switched on with the next message (`send_message` with `skills`). Closed
     sessions are read-only replays from `session_events`, marked "read-only" in the header, with
     no follow-up box. `out:N` in an observation opens the full output via `read_output`.
   - *Settings*: per provider a password field with Save / Remove (`set_key`), the equivalent
     environment variable, and defaults (`set_defaults`). Never show stored keys. Then three
     sections that edit text files, each a monospace text area with Save (enabled, beside the word
     "Unsaved", only while the text differs from what is stored; a draft survives leaving the view)
     and engine errors shown under the text:
     - **Skills**: a row per skill (title, and Built-in, Edited or Custom). A row opens the skill's
       file under it. Saving a built-in stores the user's version of it (Edited); Reset on an
       edited one brings the built-in back, Delete removes a custom one. "New skill" opens a file
       with the front matter (`name`, `title`, `description`) to fill in.
     - **System prompt**: the template in effect, with Reset while it is the user's own.
     - **Instructions**: the user's `AGENTS.md` for every project.

     Reset and Delete take a second click ("Confirm"). The data folder is the last line.
3. **Inspector** (empty unless a session is open): the caption buttons, then Economy from `stats` events (turns, steps, surprises, predictions held of made,
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

`docs/screenshots/` holds the views as that command shows them, with these in place of
`PRESAGE_UI_AUTODEMO=1` (a keyed view also takes `LOGFARE_API_KEY=x`; no session is started with it):

| File | Hooks |
| --- | --- |
| `new-session.jpg` | `PRESAGE_UI_VIEW=new`, no key |
| `new-session-model-menu.jpg` | `PRESAGE_UI_VIEW=new PRESAGE_UI_SCRIPT='click:#pick-model'` |
| `new-session-skills-menu.jpg` | `PRESAGE_UI_VIEW=new PRESAGE_UI_SCRIPT='wait:800;;click:#pick-skills:not(:disabled);;click:#pick-skills-menu [data-value="debug"];;click:#pick-skills-menu [data-value="tests"];;key:#pick-skills-menu=>ArrowDown'` |
| `session.jpg` | `PRESAGE_UI_AUTODEMO=1` |
| `session-skills.jpg` | `PRESAGE_UI_SCRIPTED=<file>` with `"skills": ["debug", "tests"]` and two turns, then a follow-up sent with a third skill (`click:#pick-skills`, `click:#pick-skills-menu [data-value="ui"]`, `type:#followup=>…`, `click:#send-followup`) |
| `settings.jpg` | `PRESAGE_UI_VIEW=settings` |
| `settings-skills.jpg` | `PRESAGE_UI_VIEW=settings PRESAGE_UI_SCRIPT='wait:1200;;click:[data-skill="tests"];;wait:400;;scroll:#settings-skills'` |
| `settings-system-prompt.jpg` | `PRESAGE_UI_VIEW=settings PRESAGE_UI_SCRIPT='wait:1500;;scroll:#settings-system'` |
| `settings-instructions.jpg` | `PRESAGE_UI_VIEW=settings PRESAGE_UI_SCRIPT='wait:1500;;type:#global-instructions=>…;;scroll:#settings-instructions'` |

The settings shots ran against a `PRESAGE_HOME` holding a user `skills/tests.md` (Edited), a custom
skill with a long title, and an `AGENTS.md`.
