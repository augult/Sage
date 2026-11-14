# Desktop browser regression fixture

This dependency-free Node server serves the **actual** files in `desktop/ui/`.
Only responses for `index.html` are modified: the server injects `fixture.js`
before `js/main.js`. No production entry point or production IPC code changes.
The fixture implements synthetic Tauri IPC and window APIs in the browser.
It does **not** exercise the Rust backend, real files, Git, network providers,
streaming, native windows, or API credentials.

## Run

```sh
node desktop/tests/serve.mjs
```

The listener defaults to `0.0.0.0:4173`; set `PORT` to choose a free port.
In Reason Cloud use
`preview_start({ command: 'PORT=4187 node desktop/tests/serve.mjs', port: 4187 })`
instead of starting a background shell process.

Open either route in a browser:

- `http://localhost:4173/` — New session, with the sample Hello session listed.
- `http://localhost:4173/?view=session` — Hello in `/home/user/Novel`, including
  its initial user message.
- `http://localhost:4173/?test` — run DOM assertions, then return to New session.
- `http://localhost:4173/?view=session&test` — run the same assertions, then
  return to Hello.
- Add `&scheme=dark` to exercise the dark appearance (light is the default).
- `/?test&audit` — original regression suite, then workspace layout and tooltip assertions.
- Add `&rich` for synthetic prose, code blocks, edit diffs, skill cards, a file tree/editor,
  long model/path labels, and GitHub changes/commits. This is all **mock data**;
  the preview cannot perform real Git, filesystem or provider operations.

The audit suite reports `window.uiAuditResults` and emits `ui-audit-complete`.
It checks Settings overflow, provider address/key readability, field bounds,
provider dialog scrolling/actions, composer hit targets, viewport-bounded menus,
absence of native titles, and (with `rich`) file-editor and GitHub layout.
Use `/?test&audit&rich` at 940×600 and 1320×860 in both appearances.

Tests wait for app mounting and fonts, use real navigation and tab clicks,
and compare rendered element bounds/computed styles. A failed assertion is
recorded without preventing later independent assertions from running.
Inspect `window.uiTestResults`: `status` becomes `complete`, `checks` holds
named `{ name, pass, detail }` entries, and `passed`/`failed` provide totals.
The root element also has `data-ui-tests="passed|failed|error"`, and the
browser emits `ui-tests-complete` with the results. These are not console-only
tests. `window.uiFixture.calls` exposes every synthetic IPC call with the
current app view so no-session filesystem/Git requests are detectable.

A browser runner can wait without timing-dependent sleeps:

```js
const result = await new Promise(resolve => {
  if (window.uiTestResults.status === 'complete') resolve(window.uiTestResults);
  else window.addEventListener('ui-tests-complete', event => resolve(event.detail), { once: true });
});
```

The sample session is resumable so its composer is exercised. Checks cover
value-only Mode/Autonomy/Effort controls, preserved accessible setting names,
changing and restoring every New-session setting, value-only active-session
Autonomy/Effort controls; no-session Economy/Code/GitHub hit targets and empty text;
no `list_dir`/`git_state` requests outside a session; repeated New session,
Settings and Hello navigation with exactly three tabs and one resizer; title
alignment to the visible folder chevron path (within 0.1px, not the SVG's
empty gutter); symmetric sidebar row outer insets of at least 12px; symmetric
File menu padding with View checkbox gutters preserved; active-session tab
rendering; and no overlap plus at least 12px of horizontal clearance between
the Hello user bubble and the inspector toggle, independent of scroll position.

Useful viewport checks include 1320×860, 1208×601, and 940×700 (the native
window's minimum width), with both
initial routes and appearances. The fixture uses no package dependencies.

## Syntax checks

```sh
node --check desktop/tests/serve.mjs
node --check desktop/tests/fixture.js
find desktop/ui/js desktop/tests -type f \( -name '*.js' -o -name '*.mjs' \) -exec node --check {} \;
npx --yes eslint@9 --config desktop/tests/eslint.config.mjs desktop/ui/js desktop/tests
npx --yes stylelint@16 'desktop/ui/styles/*.css' --config desktop/tests/stylelint.json
```
