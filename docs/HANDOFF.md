# Handoff: Presage

Read this first if you are picking the project up. It is written for someone with no prior context
and replaces every earlier version. State: 11 November 2026, end of the fourth session, which Lars
asked to wrap up before his usage limit reset.

## What the user asked for

Lars (repo owner) asked for:

1. An agent harness built from scratch with an outstanding performance/cost ratio, **something new
   that has not been made before**. Stated again this session: the goal is to beat known harnesses
   on performance and, if possible, cost, "or at least just better performance".
2. Tuned for the models he mainly uses: **DeepSeek V4.1 Flash and GLM-5.3** (cheap, think for long).
3. **A desktop app**, with a simple but modern UI. He runs **Windows 10**. This session he asked for
   a complete rework: a translucent glass look **drawn by our own code** (the system acrylic lags
   his PC), very close to a reference image of a dark three-pane coding-agent app, the **Inter**
   font, and a set of "anti AI slop" rules he pasted (no purple or gradients, no pills, no card
   grids, no hype copy, reduced-motion support, 4.5:1 contrast). `docs/DESKTOP_UI_BRIEF.md`, "Look",
   is how those rules were applied. Inter and glass are his explicit choices where they differ.
4. **Rust for everything possible.**
5. Live testing through **logfare.ai**. It logs every prompt: synthetic tasks only.
6. Subagents routed to `claude-fable-5-2`. The delegation tool takes a model id only.
7. New this session: **look at a memory space, or the agent making its own skills, but research it
   before building anything**. He prefers something that works out of the box.
8. New this session: **write the best possible system prompt, guidelines for how to go at a
   project, design guides and anti-slop rules.** He said the agent is free to steer the project as
   long as the goal is reached. None of this is written yet (see "not done", item 3).
9. He approved installing external harnesses in the sandbox for head-to-head runs ("whatever is
   useful"), and Logfare spend up to about $50 in total ("it's free, but don't run them forever").

He is terse, steers often, watches his usage (work in few turns) and wants candid status: report
numbers whichever way they fall. Ask before installing anything else beyond Rust 1.99.1, the apt
packages in `scripts/setup-dev.sh` and the crates in `Cargo.lock`.

## Where everything is

- Repo `augult/harnessnew`. This session's branch is **`hoplite/taras-be4cfe44`**, with a pull
  request into `main`. It contains everything: the previous session's follow-up results and handoff
  (pull request #2, still open, is fully contained in it and can be closed once this one merges),
  the shell fix, the UI rework, the research record and this file. GitHub's default branch is
  still `hoplite/axos-0f41d603`: a repository setting the agent's token cannot change.
- Two background agents worked in their own worktrees on branches off `main` at `b7abab2`:
  **`hoplite/taras-be4cfe44-harness-core`** (engine) and **`hoplite/taras-be4cfe44-harness-bench`**
  (benchmark). Their state is in "Background branches" below. Neither is merged.
- `crates/presage-core` (engine), `crates/presage-cli` (`presage` binary; `src/bench/` is the
  benchmark runner and `bench/<task>/{workspace,hidden,solution}` its tasks), `desktop/src-tauri` and
  `desktop/ui` (Tauri 2.12 shell, static front end), `.hoplite/setup.sh`, `scripts/setup-dev.sh`.
- `README.md` (design, status, headline numbers), `docs/BENCHMARKS.md` (every number and how to
  reproduce it), `docs/results/` (raw results), `docs/DESKTOP_UI_BRIEF.md` (UI specification),
  `docs/RESEARCH.md` (what was learned about memory, skills and other harnesses, with sources),
  `docs/screenshots/`.
- Start reading at `crates/presage-core/src/lib.rs`, then `script.rs`, `agent.rs` (`opening`
  builds the first message), `watches.rs`, `attach.rs`, `harness.rs`. For the UI: `desktop/ui/js/
  main.js`, `glass.js`, `views/`, and `styles/base.css` for the tokens.

## The design in one paragraph

On the target models cached input is cheap (DeepSeek Flash: $0.003 / $0.15 / $0.60 per M for cache
hit / miss / output; GLM-5.3: $0.26 / $1.40 / $4.40), so the scarce resource is deliberation (turns
× thinking), and the typical failure is an unnoticed error that compounds. Presage's answer is
*expectations*: all tool calls of one reply run as a script that halts at the first surprise;
`shell` calls carry predictions and a predicted success is answered with a receipt instead of the
output; watches are standing checks the harness re-runs after every change; `finish` requires a
passing check since the last change. What turned out to matter most is the harness doing the
obvious itself: the first test command the model runs becomes a watch (auto-watch), a test command
the task names is run before turn one (preflight), and the files the task names are in the first
message (attach). Every mechanism is a flag in `Features` (`state.rs`); `--classic` turns all of
them off and `presage bench --variants no-<mechanism>` turns one off.

## What is done and verified

On this branch `cargo test` runs **166 tests, all passing** (159 core, 6 CLI, 1 desktop; the
desktop crate is tested with `cargo test -p presage-desktop --target-dir target/desktop`).

From earlier sessions (numbers in `docs/BENCHMARKS.md`):

- **Baseline benchmark**, 7 tasks × 5 runs × 4 variants per model, all 280 runs solved. Median
  turns 3 for Presage against 6 for classic on both models. GLM-5.3 cost $0.0077 against $0.0088;
  DeepSeek $0.0008 against $0.0009 at the median but 17% higher at the mean.
- **`attach` and `short_paths`**, 140 runs per model: a median run takes 2 turns with attach.
  DeepSeek costs $0.0006 against $0.0011 for classic (−42%). GLM-5.3 only matches classic at the
  median ($0.0084), because it writes twice the output tokens; total spend is 12% lower.
- `presage bench --check`: the 7 easy tasks fail as shipped and pass with their solutions.
- `.hoplite/setup.sh` works in a sandbox (it did again this session, from cold).

From this session:

- **A real engine defect, found in live benchmark logs and fixed** (commit `9256285`,
  `tools/jobs.rs`, with a test). Every command an agent runs is a fresh `bash` that inherited
  `OLDPWD` from whatever launched Presage. A model that ran `cd -` or `cd "$OLDPWD"` landed in an
  unrelated directory; the shell's directory persists between commands and the file tools follow
  it, so the session stayed lost. One DeepSeek session jumped from its temporary workspace into
  the repository checkout, searched the whole disk, read another task's hidden checks, wrote
  files into the checkout and ended at the 30-turn budget at about 40 times the usual cost. 10 of
  517 session logs from this session's benchmark runs name the checkout. Commands now start with
  `OLDPWD` set to their own directory. **Benchmark numbers produced before this fix in this
  session may include such runs** (see "not done", item 2).
- **Desktop UI rework** (commit `c33f810`; described in `docs/DESKTOP_UI_BRIEF.md`, screenshots in
  `docs/screenshots/`). Undecorated window with its own title bar and caption buttons; glass made
  once at start-up from the blurred wallpaper and pinned to the screen (no `backdrop-filter`, no
  system acrylic); Inter bundled; New session is one composer with pickers for model, effort,
  mode and autonomy; sessions grouped by day. Verified headless under Xvfb on Linux:
  - every view renders with an empty error log (new session with and without a key, model and
    mode menus open, the walkthrough session, settings);
  - the close button exits the app (so the window capability works);
  - moving the window by 160px leaves the backdrop where it was on the screen: 0.07% mean
    difference for the same screen rectangle, 3.3% had it moved with the window;
  - text contrast computed over a 5,832-colour grid of possible wallpapers: 4.8:1 or better for
    every ink and surface combination the styles use.
  **Not verified, because there is no Windows machine here:** reading the wallpaper from
  `%APPDATA%\Microsoft\Windows\Themes`, dragging by the title strip, minimise and maximise, the
  resize border of the undecorated window, per-monitor wallpapers, and how it feels at 60 Hz.
  Also not exercised after the rework: a live session, the approval card, the folder picker and
  the output viewer (their code did not change beyond class names, but nobody looked).
- **Research**, in `docs/RESEARCH.md`: how memory and self-made skills work elsewhere and what the
  evidence says; how to run ten other harnesses against Logfare for a head-to-head; seven
  mechanisms worth adopting.

## Decisions taken this session

So that they are not argued again without new evidence (the evidence is in `docs/RESEARCH.md`):

- **Memory and skills.** Ship built-in, hand-written playbooks that the harness selects (at most
  two per session, appended to the first message so the cached prefix stays byte-identical).
  Learn only facts the harness can verify mechanically, such as the check command that passed.
  Keep project notes human-written (`AGENTS.md`, `CLAUDE.md` and `.presage.md` are already read).
  Do not build model-written memories or skills, vector memory, memory blocks inside the system
  prompt, or a self-rewriting harness: measured effects are zero or negative, most of all on
  small models, and the first two would need many runs before paying off.
- **Claims against other harnesses need other harnesses.** Until one runs on the same tasks and
  models, Presage has only been compared with its own classic loop.
- **UI.** Neutral glass, colour only for meaning, Inter. Rules in `docs/DESKTOP_UI_BRIEF.md`.

## Background branches

Both branch off `main` at `b7abab2`, so they have neither pull request #2's files nor the shell
fix. They were still running when this was written; the commits below existed at that point. Check
`git log main..<branch>` for more, and expect each to end with result files under `docs/results/`.

- **`hoplite/taras-be4cfe44-harness-core`**: `a14a8b1` "Let a change or a check carry the finish
  summary; show short files whole where a failure points into them" (aimed at the separate
  `finish` turn and at cost). Not reviewed and its measurements not seen by the parent.
- **`hoplite/taras-be4cfe44-harness-bench`**: `2d82b71` (attach measurement on GLM-5.3) and
  `cb327d2` "a hard set of eight tasks, selectable with `--tasks easy|hard|all`" (`catalog`,
  `invoice`, `ledger`, `plugins`, `quotes`, `recur`, `sync`, `vercon`). Not reviewed.

Merging them: the bench branch re-measured attach without knowing pull request #2 had the complete
results, so `docs/results/2026-11-11-*-attach*` and `docs/BENCHMARKS.md` will conflict. Keep this
branch's versions of the attach result files; take the tasks and the runner changes. Both branches
touch `crates/presage-cli/src/bench/mod.rs`.

## What is not done, in priority order

1. **Review and merge the two background branches.** Read the diffs, run `cargo test` and
   `presage bench --check`, and read their result files before trusting any number.
2. **Measure again after the shell fix.** Before quoting any live number from this session, count
   contaminated sessions per cell: `grep -rl "<checkout path>" <bench home> --include=events.jsonl`
   (the bench homes were under `/tmp` and are gone, so in practice: re-run). A lost session ends
   at the turn budget, which makes whichever variant it hit look worse.
3. **The texts Lars asked for (item 8 above).** Not started. Write, then measure each against a
   variant without it:
   - the system prompt, rewritten for small models and kept as a byte-stable prefix (it is in
     `prompt.rs`; the core branch may have shortened it);
   - playbooks in a new `playbooks.rs`: debugging a failing test, adding a feature, refactoring,
     starting a project from nothing, UI design with the anti-slop rules, per-ecosystem
     conventions. About 200 words each, selected from task keywords and workspace markers, with a
     `playbooks` flag in `Features` and a `no-playbooks` bench variant.
4. **Keep sessions inside their workspace.** The shell fix closes one door. `adopt_cwd` in
   `tools/shell.rs` still follows a `cd` to anywhere, and the file tools go with it; a model can
   still `find /`. At least refuse to adopt a directory outside the workspace root and tell the
   model. For the benchmark this is also about validity: hidden checks and reference solutions
   sit in the source tree on the same disk as the runs.
5. **Head-to-head with other harnesses.** `docs/RESEARCH.md` has the commands. Start with
   mini-swe-agent (cheapest, the usual baseline), as an external variant in `bench/mod.rs` that
   spawns a command in the materialised workspace and reuses `verify`.
6. **Verified project facts and a multi-session benchmark** (`presage bench --series`), as
   specified in `docs/RESEARCH.md`. Also `presage remember "<text>"`.
7. **Mechanisms from other harnesses**, in the order of `docs/RESEARCH.md`: reject and revert
   edits that break syntax, a red-then-green gate, verified retries, a repo map.
8. **The desktop app on Windows 10**: everything listed as not verified above, then a session
   against a real provider started from the app, closing the window while a step runs, a release
   build, clippy.
9. **Tidy the record.** `docs/BENCHMARKS.md` has the follow-up tables appended below a paragraph
   that still calls them unfinished, and the README table shows the baseline only. The bench
   branch may have rewritten `docs/BENCHMARKS.md`; reconcile.
10. **Cost on GLM-5.3** (it writes twice the output tokens when it edits in its first reply) and
    **the separate `finish` turn**; the core branch works on both.
11. Direct DeepSeek, Z.ai and OpenRouter dialects are untested (no keys). Smaller gaps: sessions
    cannot be resumed after an app restart; Windows shell handling; no OS sandbox for commands;
    effort ratchet and lesson recall are tested with the scripted model only. Engine wishes from
    the UI work, none implemented: `Harness::events` drops lines it cannot type; no orderly
    shutdown; `ProviderInfo` cannot say whether a key is stored or from the environment;
    `Approval` does not name its step; no event when a watch is switched off.

## Facts established by live probing (do not re-derive)

- Logfare: OpenAI-compatible at `https://logfare.ai/v1`, bearer key, OpenRouter-shaped (`reasoning:
  {effort}` + `include_reasoning: true`; usage in a final chunk). Models that answer: `glm-5.3`,
  `deepseek-v4.1-flash-1017`, `economy/deepseek-v4.1-flash` (slow, uncached, three times the price).
- **The limit was measured at 80 requests a minute for the whole account.** GLM-5.3 at `-j 3` stays
  under it. DeepSeek at `-j 3` goes over (HTTP 429, retry in 25 s; the harness waits and retries).
  Run one benchmark at a time, DeepSeek with `-j 2`. Logfare's own documentation says 40 a minute
  and 6 concurrent; the two have not been reconciled.
- A 140-run benchmark takes about 20 minutes and $2.50 on GLM-5.3 (a few `roman` runs think for
  minutes) and about 7 minutes and $0.16 on DeepSeek.
- GLM-5.3 returns several tool calls per reply. Its cache through the gateway is erratic (58–73%
  cached at the median with a byte-stable prefix), which is why the "no cache" column exists.
- Before attach a Presage run was `read → edit → finish` on both models. `finish` in the same reply
  as the edits: 6 of 35 runs on GLM-5.3, 1 of 35 on DeepSeek.
- With a watch in place the model makes almost no predictions of its own (11 to 14 in 35 runs,
  against about 50 without auto-watch).
- All seven easy tasks name the check or the file, which is exactly what preflight and attach use.
  A comparison with another harness needs tasks that do not.

## Sandbox notes that cost time

- The thread starts on its own branch. Fetch this one with an explicit refspec and check it out
  without `--track` (the clone is single-branch):
  `git fetch origin refs/heads/hoplite/taras-be4cfe44:refs/remotes/origin/hoplite/taras-be4cfe44`,
  then `git checkout -b hoplite/taras-be4cfe44 origin/hoplite/taras-be4cfe44`. The same for the two
  background branches.
- If `pkg-config --modversion webkit2gtk-4.1` fails, setup did not install the libraries; do it by
  hand as root:
  `apt-get update -qq && apt-get install -y -qq --no-install-recommends libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev pkg-config build-essential xvfb imagemagick`.
  `cargo test` does not build `target/debug/presage`; run `cargo build` first.
- Tool output over about 2,000 tokens is replaced by a lossy summary: read files in ranges of at
  most 60 lines and trim command output.
- The command policy blocks: piping into an interpreter or a heredoc into one (`python3 - <<EOF`),
  running or even syntax-checking a shell script you wrote (`bash x.sh`), a variable as the
  command, arithmetic around a command substitution (`$(( $(date +%s) - T ))`; use `SECONDS`),
  heredocs or loops in a command that also calls `git` or `gh`. Write files with the file tool,
  then run plain commands. `node file.mjs` and a compiled C helper are fine.
  Push with an explicit refspec: `git push origin HEAD:refs/heads/hoplite/taras-be4cfe44`.
- Long runs: copy the binary first (`cp target/debug/presage /tmp/presage-bin/…`) so that a rebuild
  cannot change a benchmark under way, and start it with `nohup setsid … > report 2> progress &`.
- `/tmp` does not outlive the session. Commit results as soon as they exist.
- Subagents share the parent's files, and **their shells start in the parent's checkout** with
  `OLDPWD` pointing at it. Give each its own `git worktree`; build the desktop crate with
  `--target-dir target/desktop`. A subagent cannot be messaged or stopped from the parent, and it
  keeps running when the user interrupts the parent. Stage explicit paths, never `git add -A` at
  the root: stray files from their runs turned up in the checkout twice.
- Screenshots of the app: the command at the end of `docs/DESKTOP_UI_BRIEF.md`, with
  `PRESAGE_UI_WALLPAPER=<image>` to put a picture behind the glass and `LOGFARE_API_KEY=x` to see
  the views as they are with a key (start no session with it). Xvfb has no window manager, so
  maximise and minimise cannot be tried there; moving the window took a ten-line Xlib program.
- The Logfare key is not in the repository and must never be. `LOGFARE_API_KEY` is not set as a
  project variable; Lars pasted a key in chat (for the third time). It should be rotated and set
  as a project variable.

## Suggested first moves

1. Fetch the three branches, `cargo test`, `cargo build`.
2. Read `docs/RESEARCH.md`, then review the two background branches (item 1).
3. Write the system prompt and the playbooks (item 3) and measure them on the hard task set once
   it is merged, after confining sessions to their workspace (item 4).
4. Add mini-swe-agent as an external baseline (item 5).
