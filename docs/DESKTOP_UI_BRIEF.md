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
Dependencies are limited to what `Cargo.toml` lists (`tauri-plugin-dialog` is there for the
system's folder dialog). The window is undecorated (`decorations: false`, `shadow: false`, so
Windows 10 draws no white frame line): the UI draws the title bar, and the capability lets the
window drag, minimise, maximise and close itself. A request to close the window, from its own
button or from the system, goes to the page first (`onCloseRequested` in `ui/js/chrome.js`), which
asks about unsaved text and then destroys the window.

Commands, each `Result<T, String>`: `app_state`, `set_key(provider, key)`,
`set_defaults(provider?, model?, effort?)`, `list_sessions`, `start_session(req)` (force
`interactive = true`; `req.skills` names the skills to start with), `start_demo`,
`send_message(id, text, skills?, changes?, files?)` (`send_with`: the skills named are switched on with the
message; `changes` is `{ provider?, model?, effort?, autonomy? }`, what the session is to run with
from this message on), `stop_session(id)`,
`approve(id, approval, allow)`, `session_events(id)`, `read_output(id, handle)`,
`pick_folder(start?)` (the system's folder dialog, opened at `start`; the chosen path, or nothing),
and `subscribe(id, on_event: tauri::ipc::Channel<Event>) -> Vec<Event>`, which returns the backlog
and forwards live events until the channel or broadcast closes (keep going on `Lagged`). The UI
should ignore events whose `seq` it has already applied.

Skills and instructions: `list_skills(cwd?)` (built-in and user skills, plus the project's own when
a folder is given), `skill_file(name, cwd?)` (the whole file, front matter included),
`save_skill(name, content)`, `delete_skill(name)` (a user skill; one that replaced a built-in gives
the built-in back), `instructions()` (`{ system, system_default, system_custom, global }`),
`set_system_prompt(template?)` (nothing resets to the built-in) and `set_global_instructions(text)`
(empty deletes the file).

For the GitHub tab: `git_state(id)` (where the session's workspace stands: `{ repo, branch, remote,
url, github, upstream, ahead, files, commits }`, `github` being `owner/name` when the remote is on
github.com, `commits` the latest twelve as `{ hash, subject, when }`, of which the first `ahead`
are unpushed) and
`git_push(id, message)` (commits what is uncommitted under `message`, then pushes the branch,
setting its upstream; `{ commit, remote, branch, pushed }`). Both run the user's own `git`, with
the user's credentials (`src/git.rs`).

For the Code tab (`src/files.rs`): `list_dir(id, path?)` (what a directory of the session's folder
holds, folders first, without `.git`: `[{ name, path, dir }]`), `read_file(id, path)` (its text; a
file over 1 MB or not UTF-8 is refused) and `write_file(id, path, text)` (to a file that is there
already). Paths are from the folder's root, and nothing outside it is reached.

For attachments: `pick_files(start?)` (the system's file dialog; the chosen paths, or none).
`start_session` takes them as `req.files` and `send_message` as `files`: each must be a file that
is there. The engine names them in the `user` event and puts their paths in front of the message
for the model, which reads the ones it needs with its `read` tool.

GUI apps start with a minimal `PATH`. On unix, first thing in `main`, resolve the login shell's
`PATH` (`$SHELL -ilc 'printf %s "$PATH"'`, 3 s timeout) and adopt it.

Headless test hooks via environment: `PRESAGE_UI_AUTODEMO=1` (start and open the demo),
`PRESAGE_UI_VIEW=settings|new|session`, `PRESAGE_UI_OPEN=<session id>`. Added while building it:
`PRESAGE_UI_SIZE=<width>x<height>`, `PRESAGE_UI_SCRIPTED=<file>` (start a session played by a
scripted stand-in and open it), `PRESAGE_UI_SCRIPT` (UI actions to play once the first view is up;
grammar in `desktop/ui/js/hooks.js`), `PRESAGE_UI_THEME=<background>` and `PRESAGE_UI_SCHEME=light|dark`
(the painting behind the glass and the appearance, for that launch only).
A scripted session's file may name `skills` to start with.
Pickers answer to scripts as `#pick-model`, `#pick-skills`, `#pick-effort`, `#pick-mode`,
`#pick-autonomy`, and their options as `#pick-mode-menu [data-value="classic"]`; a model's option
is `#pick-model-menu [data-value="<provider>:<model>"]`. The Skills button
is disabled until its list has loaded: click `#pick-skills:not(:disabled)`. It opens `dialog.skills`,
where a skill's card is `[data-skill="debug"]` with `.skill-use` and `.skill-view` in it. A chat bar has
`#attach` (a script attaches by path, `attach:#attach=><path>,<path>`: the system's dialog cannot be
driven), the box `#task` or `#followup`, and `#start`, `#send-followup` or `#stop-session`. The
inspector's bar is `#tab-economy`, `#tab-code` and `#tab-github`; in Code a row of the tree is
`[data-path="src/board.js"]`, the text `#code-text` and its button `#code-save`; in GitHub the
message is `#push-message` and the button `#push-go`. In Settings:
`#settings-skills`, `#settings-system`, `#settings-instructions`, a skill's row as
`[data-skill="debug"]`, `#new-skill`, the text areas `#skill-file`, `#system-prompt`,
`#global-instructions`, `#settings-keys` with a provider's row as `[data-provider="openai"]`,
`#settings-defaults` with `#default-provider` and `#default-model`, and in the reminder of unsaved
text `#unsaved-save` and `#unsaved-discard`.

## Front end (`desktop/ui`)

Static files, no bundler, no framework, no external resources. `window.__TAURI__.core.invoke` and
`new window.__TAURI__.core.Channel()`. Never put event, model or tool text through `innerHTML`.

### Look

Plain glass over a painting: dense, quiet, and without a hue of its own.

- **Type.** Inter, bundled (`ui/fonts/`, SIL OFL), at four sizes (11, 12, 13, 14 px) and three
  weights (400, 500, 600). The system monospace for paths, commands and observations only.
- **Shape.** 8px grid. Corners are barely turned: 2px tags and marks, 3px controls and rows, 4px
  cards, menus, dialogs and the sheet. Nothing is a pill; only what sends, stops or attaches in a
  chat bar is round, a glyph on a 28px disc with no word beside it. Every button is 28px tall, and a
  one-line field beside one is as tall; the small buttons inside a line or a header (copy, a
  diff's layout, a recent folder) are 24px. A dialog is closed with a cross on light red, which
  darkens when pointed at.
- **The interface has no colour.** Ink, paper and the greys between, in a light and a dark
  appearance (`data-scheme` on `<html>`, chosen in Settings or taken from the system). Colour
  belongs to the painting behind the glass and to two things: a failure, which must not be
  missed (`--danger`: a tool that failed has its whole line in red), and an edit, green for what
  it added and red for what it removed
  (`--add`, `--del`: the `+5 −1` counts, and behind the lines of a diff). A state is a word or
  a glyph ("surprise", "failed", "skipped", "held"); a tone (`.tone-*` in `base.css`) only
  decides how much ink it gets. Tools are otherwise grey: the bar and the rows. The primary
  button is ink on paper, one per view. No gradients, glows or accent borders.
- **Boxes are rare.** In the conversation only the user's message, the bar of a stretch of work,
  an approval request, verbatim output and diffs are boxed; the answer is plain text.
- **Motion.** A view or dialog arriving fades in over 140 ms. A fold slides open and shut over
  180 ms while its chevron turns a quarter. The dither drifts at 20 frames a second. Motion off
  (Settings, or `prefers-reduced-motion`) holds the dither still and removes the rest; Dither
  off takes the dots away altogether and leaves the painting.
- **No explanatory text.** A label, a value, a state ("Read-only", "No watches", "Built-in") or an
  error is shown; a sentence that explains how the product works is not, in a note, a caption, a
  placeholder or a tooltip. Placeholders are a word or two ("Folder", "Task"), empty states two or
  three. Where a control is unclear without a sentence, the control is changed. What the engine
  itself reports about a provider (a gateway that logs prompts) is data: it is the `title` of the
  provider's name in Settings, not a line of its own.
- **Title bar.** The top 44px of each pane is the title bar (`data-tauri-drag-region`), with the
  caption buttons where Windows puts them. A session has no strip over its conversation: the
  window drags by the sidebar's and the inspector's.
- **Icons.** Every glyph is drawn (`ui/js/dom.js`, and the caption buttons in `index.html`) with
  round ends and turned corners: a vertex is a small arc, a shape's corners are quarter circles.

### Glass (`ui/js/backdrop.js`, `ui/js/dither.js`, `ui/js/themes.js`, `ui/styles/glass.css`)

No `backdrop-filter`, no system acrylic and no window transparency: the first two blur again on
every frame, which is what makes acrylic windows lag on Windows 10, and the window never shows
what is behind it. The app brings its own picture. A background is a painting made by a script
(`desktop/art/<id>.py`, numpy and Pillow, deterministic; it writes `ui/themes/<id>/art.webp` at
2400×1500 and `thumb.webp` at 480×300): Ember, Orchid, Tide and Graphite, or None for flat panes.
At start-up, and on every change of background or appearance, `backdrop.js` takes the painting
at 320px, softens it, presses its brightness into a band (14–76 of 255 for dark, 198–248 for
light) and draws it once, with a little grain, to a canvas behind the panes. Over it a second,
much smaller canvas carries an ordered dither (an 8×8 Bayer matrix over three slow waves, set per
background in `themes.js`), stretched without smoothing so that its dots stay square. The dither
shows on the glass of the sidebar and the inspector only: it is cut away where the sheet lies
(`clip-path` in `glass.css`, from the one pane width in `layout.css`). The panes
are translucent fills: the veil over everything, and in the middle the sheet, the reading
surface. Nothing blurs or filters while the app runs. `prefers-reduced-transparency` is read as
None.

The three choices (appearance, background, motion) are kept in `localStorage` (`ui/js/prefs.js`)
and put on `<html>` before the first paint (`ui/js/boot.js`).

Contrast is part of the material. Over a 5,832-colour grid of what the pressed backdrop can be,
with its grain and under a dither dot, the lowest ratios are:

| Surface | `--ink` | `--soft` | `--muted` | `--faint` |
| --- | --- | --- | --- | --- |
| Light, glass (sidebar, inspector) | 10.8 | 8.1 | 5.5 | 3.5 |
| Light, sheet | 17.5 | 11.3 | 6.6 | 3.3 |
| Dark, glass | 7.1 | 5.9 | 4.7 | 3.2 |
| Dark, sheet | 15.5 | 11.1 | 7.4 | 4.5 |

So `--ink`, `--soft` and `--muted` keep 4.5:1 everywhere; `--faint` is for marks, placeholders
and key hints. On the sheet `--danger` keeps 6.2:1, `--add` 4.7:1 and `--del` 6.1:1. Change a band in `backdrop.js`, a pane's
fill, a dither ink or an ink colour, and these have to be recomputed.

### Panes

1. **Sidebar**: app mark and name (the strip drags the window), New session, a tab per folder
   (chevron and the folder's name; the folders stand in the order of their newest
   session, each opens onto its sessions and stays shut once shut, kept by the window; a shut
   folder shows the mark of a session in it that is running or waiting for an approval; going to
   a session opens its folder; the walkthrough's throwaway workspaces share one folder, "demo"),
   under each its sessions, and Settings. A session is one line: its title, and after it a mark
   only while it has something to say, a dot with a ring that swells while it runs, a triangle
   when it waits for an approval or, in red, when its run failed. A session's row is as wide as
   its folder's, and its title starts as far in from the row's left as its mark ends from the
   right. The sidebar and the inspector are the
   same width, until the inspector shows Code, which widens it.
2. **Main**:
   - *New session*: no title and no picture. At the foot of the sheet is the chat bar, built
     by the same function as a session's (`chatBar` in `ui/js/views/common.js`) so that it is
     in the same place once the first message is sent: a box with the task and Start, and under
     the box the pickers. In the middle of the sheet, a little above its centre, is the folder:
     the folders of up to five recent sessions by name (the full path is the `title`), the
     chosen one checked, and under a rule "Choose folder…" (the system's own folder dialog,
     `pick_folder`); a folder picked there joins the list. Under the box,
     pickers for model, skills, effort (Auto, Low, High, Max: Auto is the engine's `ratchet`,
     which starts high and steps up when predictions fail), mode (Presage / Classic) and autonomy
     (Auto / Ask). Start is Enter in the task, or Ctrl/Cmd+Enter if Settings says so; the
     other of the two breaks the line, as Shift+Enter always does; the key is not written beside the box. The model menu lists models under their provider
     from `app_state`, the providers that have a key first; one without a key has its models
     greyed and disabled. A menu is as wide as its words, and the check of what is chosen sits at
     the end of its line. Paths are shown without Windows' `\\?\` prefix. The Skills button sits next to the model and
     opens a dialog of the skills of the chosen folder (`list_skills`) as cards, two to a row:
     on each its title, its description, Use (which reads "In use" once on; several can be on),
     View (the whole skill in place of the cards, with a way back) and a button that copies its
     text. Nothing is sent by choosing: a skill in use goes with the task, or in a session with
     the next message. The button reads "Skills" with nothing on, then "Skills" and the number on
     (the titles are its `title` attribute), so the bar keeps to one row. A
     new session starts with no skills, whatever the last one had; Classic takes none, so the
     button is disabled there. With no key configured, the view leads with "Add an API key", and
     Start is not the primary button. The walkthrough (`start_demo`) has no button: it is played
     by `presage demo` and by the `PRESAGE_UI_AUTODEMO` hook.
   - *Session*: no header: what the session is (title, status, folder, model, effort, mode,
     autonomy, skills) is the first block of the inspector's Economy tab. The timeline is a
     conversation, and by default it shows only what was said: the user's message (a grey block on
     the right), the agent's words as prose (`ui/js/markdown.js`, `ui/js/views/prose.js`:
     headings, lists, tables, fenced code with a copy button; a link is copied, never followed),
     and how the run ended in one line ("Done · verified by watch tests") with a card of the files
     it edited. Everything else is folded (`ui/js/views/timeline.js`):
     - **One grey bar per stretch of work** is the master toggle of every tool call the agent ran
       between two things it said, across as many turns as went by without words. It says what
       was done, from the steps (`ui/js/activity.js`): "Read 2 files, ran 2 commands, edited
       report.sh". A lone file is named; a `finish` that went through is not counted, since it
       is the answer. What did not go to plan is appended, because the bar starts shut:
       "1 surprise, 1 skipped". While its last turn runs the bar has a spinner and says what is
       happening now ("Thinking 2.1s", "Running sh check.sh 0.4s").
     - **The bar opens the rows**: the reasoning ("Thought", with its first line), a
       row per step (icon, verb, summary, the state when it was not simply fine, lines added and
       removed, duration), and the notes that arrived between them (the environment snapshot,
       watch changes, harness notes, lessons, effort changes, trims, retries, answered
       approvals).
     - **A row is one line about what the tool did** ("Ran sh check.sh", "Edited report.sh",
       "Searched the web for awk printf") **and opens the same way for every tool**
       (`ui/js/views/tools.js`): **Input**, the arguments of the call as the model wrote them,
       as JSON, with a copy button; the prediction and whether it held; then **Output**, the
       observation verbatim (`out:N` in it opens the full output via `read_output`; a file's
       anchors step back into a gutter). Where the output has a shape it is set that way, with
       the verbatim text folded under it as "Raw output": an edit or a write as a before and
       after, Unified or Split (the choice is kept), green for added lines and red for removed,
       from the `change` the engine sends with `step_end` (`presage-core/src/diff.rs`); a web
       search as the sources it found (title, site, snippet, copy link).
     - **Edited files**: under the result, a card with a row per file the run edited (lines added
       and removed), each opening into its edits.
     - An approval request that is waiting is a card with Allow / Deny, never inside a fold.
     - **A provider error is said in the open.** While a turn has no reply and the provider has
       answered with an error, a red block under the bar gives the error and the attempt
       ("Provider error · retrying · attempt 2"), and the bar reads "Retrying", not "Thinking":
       the engine retries up to eight times, and a bar alone can say so for minutes. With
       Auto-retry on (Settings) it does not stop at eight: a request turned away for a reason
       that passes (a rate limit, an overload, a dropped connection) is made again, at most a
       minute apart, until it is answered or the session is stopped, so a run left overnight
       outlasts a rate limit. The request is the same one: no message is added to the
       conversation. An error that will not pass (a bad key, a refused request) still ends the
       run. The rows under the bar keep one line for a run of attempts, the latest.
     - A run the user stopped ends on its summary alone ("Stopped by the user.").
     - A row of the model's thought is led by a spark while it is at it ("Thinking") and by a
       bulb once it has ("Thought").
     - **The chat bar** (`ui/js/views/common.js`, `attach.js`): in the box, a + that attaches
       files (the system's file dialog), the text, and one round button: Send, an arrow, or
       while a run is in progress Stop, a square. Neither has a word on it. Attached files sit
       in the box over the text, each a chip with its name and a cross that takes it off, so
       the box grows upwards by their row; they go with the next message and show over it in
       the conversation. The pickers are under the box.

     Nothing is open until the user opens it, and what they opened stays open across redraws and
     on coming back to the session. Idle sessions get
     a follow-up box with the same pickers under it, less the mode: model, Skills, effort and
     autonomy. The session's skills are in use and locked, and any others picked are switched on
     with the next message (`send_message` with `skills`). The model, the effort and the autonomy
     show what the session has; another one picked goes with the next message (`send_message`
     with `changes`) and is the session's from then on, which a `settings` event records and the
     inspector then shows. The conversation is kept. With another model picked, a line over the box
     says that model will read the whole conversation, and how many tokens, none of it cached;
     with another effort, that some providers reread it. Closed
     sessions are read-only replays from `session_events`, marked "read-only" after their status
     in the inspector, with
     no follow-up box.
   - *Settings*: first Appearance (Mode: System, Light or Dark; Background: the four paintings
     and None, each as a small picture of itself; Dither: On or Off; Motion: On or Off; each
     applies at once). Then **Messages** (Send with: Enter, the default, or Ctrl/Cmd+Enter; the
     other one starts a new line; kept by the window. Auto-retry: On or Off, `set_auto_retry`,
     kept by the engine in `config.json` and applied at once to running sessions too). Then **API keys**, a line per provider: a check and the name in full ink
     when it has a key, a ring and grey when it has none; its address; a password field; and
     Remove when there is a key to remove. The key is kept (`set_key`) when the field is left or
     Enter is pressed. Never show stored keys. Then **Defaults** (`set_defaults`): the provider
     and the model as dropdowns (a provider without a key is greyed and cannot be chosen) and the
     effort; each is kept the moment it is chosen. Then three sections that edit text files, each
     a monospace text area with engine errors shown under it.

     **Nothing in Settings has a Save button, and nothing says "Saved".** Text typed into a file
     is a draft. Leaving Settings or closing the window with drafts opens a reminder
     (`ui/js/unsaved.js`) that lists them, with Save, Discard and Cancel; Ctrl/Cmd+S in a text
     area saves that file where it is.
     - **Skills**: a row per skill (title, and Built-in, Edited or Custom). A row opens the skill's
       file under it. Saving a built-in stores the user's version of it (Edited); Reset on an
       edited one brings the built-in back, Delete removes a custom one. "New skill" opens a file
       with the front matter (`name`, `title`, `description`) to fill in.
     - **System prompt**: the template in effect, with Reset while it is the user's own.
     - **Instructions**: the user's `AGENTS.md` for every project. The engine puts a line of its own ahead of it in the prompt (and ahead of a project's file), not shown here and not editable: that what follows is standing instructions, not a message in the conversation.

     Reset and Delete take a second click ("Confirm"). The data folder is the last line.
3. **Inspector** (empty unless a session is open): the caption buttons, then a bar of three
   buttons that says what the pane shows (`ui/js/views/panel.js`; the choice is kept from one
   session to the next):
   - **Economy** (`inspector.js`): Session (title, status, folder, model, effort, mode, autonomy,
     skills), then Economy from `stats` events (turns, steps, surprises, predictions held of made,
     prompt tokens with cached share, output with thinking share, cost "at list price" or "fabricated"
     when simulated, context size, model versus tool time, output kept out of context, repairs,
     retries, cache misses); Watches with current state; Notes and lessons.
   - **Code** (`code.js`): the folder's files as a tree, read a folder at a time, and the one
     that is open as text with line numbers, to edit and Save (Ctrl/Cmd+S). Text typed and not
     saved is marked by a dot after the file's name and is asked about before it is left (another
     file, another view, closing the window). After a run the tree and an unedited open file are
     read again. The pane is wider while it shows Code. There is no syntax colouring.
   - **GitHub** (`github.js`): the repository, branch and remote; every file a commit would take
     (one that is still there opens in Code) and a commit message; one button that commits what
     is uncommitted and pushes the branch, "Commit and push" or "Push"; under it the latest
     commits, the unpushed ones marked. The tab counts what is waiting (changed files plus
     commits ahead). A folder that is no repository says so.

## Build and verify

```sh
cargo build -p presage-desktop --target-dir target/desktop

PRESAGE_HOME=/tmp/presage-ui-shots PRESAGE_UI_AUTODEMO=1 \
WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 \
WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 LIBGL_ALWAYS_SOFTWARE=1 \
timeout 60 xvfb-run -a -s "-screen 0 1320x860x24" bash -c \
  'target/desktop/debug/presage-desktop & APP=$!; sleep 12; import -window root /tmp/shot.png; kill $APP; wait $APP 2>/dev/null; true'
```

Look at every screenshot. The demo session must show only the task, one grey bar, the answer, the
line for the outcome and the card of the file it edited; the bar must end "1 surprise, 1 skipped",
and its rows, opened, must show the surprise with its prediction and the skipped `finish`.
The walkthrough: reply 1 reads and runs the check (it becomes watch `tests`), reply 2 edits and
predicts a pass (a surprise; `finish` is skipped), reply 3 edits and finishes (verified by watch
`tests`).

`docs/screenshots/` holds the views as that command shows them, with these in place of
`PRESAGE_UI_AUTODEMO=1` (a keyed view also takes `LOGFARE_API_KEY=x`; no session is started with it):

| File | Hooks |
| --- | --- |
| `new-session.jpg` | `PRESAGE_UI_VIEW=new`, no key |
| `new-session-model-menu.jpg` | `PRESAGE_UI_VIEW=new PRESAGE_UI_SCRIPT='click:#pick-model'` |
| `new-session-skills-menu.jpg` | `PRESAGE_UI_VIEW=new PRESAGE_UI_SCRIPT='wait:800;;click:#pick-skills:not(:disabled);;click:[data-skill="debug"] .skill-use;;click:[data-skill="tests"] .skill-use'` |
| `session.jpg` | `PRESAGE_UI_AUTODEMO=1` |
| `session-tools.jpg` | `PRESAGE_UI_AUTODEMO=1 PRESAGE_UI_SCRIPT='wait:9000;;click:.work-toggle;;click:[data-step="2.1"] .row-line;;click:[data-step="2.2"] .row-line;;wait:600;;click:[data-step="2.1"] [data-mode="unified"];;scroll:[data-step="2.1"]'` |
| `session-diff.jpg` | `PRESAGE_UI_AUTODEMO=1 PRESAGE_UI_SCRIPT='wait:9000;;click:.changes .row-line;;wait:300;;click:.changes [data-mode="split"];;wait:400;;scroll:.result'` |
| `session-sources.jpg` | `PRESAGE_UI_OPEN=<id>` of a stored session whose log has a `web_search` step (the engine has no such tool yet: the step was added to `events.jsonl` by hand), then `click:.work-toggle` and the step's `.row-line` |
| `session-skills.jpg` | `PRESAGE_UI_SCRIPTED=<file>` with `"skills": ["debug", "tests"]`, then `wait:7500;;click:#pick-skills:not(:disabled);;click:[data-skill="ui"] .skill-use` |
| `session-github.jpg` | a session whose folder is a git repository with a github.com remote, uncommitted changes and unpushed commits, then `click:#tab-github` |
| `session-code.jpg` | the same session, then `click:#tab-code;;click:[data-path="src"];;click:[data-path="src/board.js"]` |
| `session-attach.jpg` | `PRESAGE_UI_VIEW=new PRESAGE_UI_SCRIPT='click:.project;;attach:#attach=><path>,<path>;;type:#task=>…'` |
| `session-error.jpg` | `PRESAGE_UI_OPEN=<id>` of a stored session whose log was cut after its first `turn_start` and given two `retry` events and a `finish` with status `stopped`, then `click:.work-toggle` |
| `settings.jpg` | `PRESAGE_UI_VIEW=settings` |
| `settings-keys.jpg` | `PRESAGE_UI_VIEW=settings LOGFARE_API_KEY=x OPENAI_API_KEY=x PRESAGE_UI_SCRIPT='wait:1200;;scroll:#settings-keys'` |
| `settings-defaults.jpg` | the same with `ANTHROPIC_API_KEY=x` and `scroll:#settings-defaults;;click:#default-provider` |
| `settings-unsaved.jpg` | `PRESAGE_UI_VIEW=settings PRESAGE_UI_SCRIPT='wait:1500;;type:#global-instructions=>…;;type:#system-prompt=>…;;click:#new-session'` |
| `settings-skills.jpg` | `PRESAGE_UI_VIEW=settings PRESAGE_UI_SCRIPT='wait:1200;;click:[data-skill="tests"];;wait:400;;scroll:#settings-skills'` |
| `settings-system-prompt.jpg` | `PRESAGE_UI_VIEW=settings PRESAGE_UI_SCRIPT='wait:1500;;scroll:#settings-system'` |
| `settings-instructions.jpg` | `PRESAGE_UI_VIEW=settings PRESAGE_UI_SCRIPT='wait:1500;;scroll:#settings-instructions'` |

Any of them takes `PRESAGE_UI_THEME=ember|orchid|tide|graphite|none` and
`PRESAGE_UI_SCHEME=light|dark`; the screenshots go through all four paintings in both appearances.
The settings shots ran against a `PRESAGE_HOME` holding a user `skills/tests.md` (Edited), a custom
skill with a long title, and an `AGENTS.md`.
