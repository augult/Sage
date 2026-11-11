# Handoff: Presage

Read this first if you are picking the project up. It is written for someone with no prior context
and replaces every earlier version. State: 11 November 2026, end of the third session, which was
cut short by the usage limit.

## What the user asked for

Lars (repo owner) asked for:

1. An agent harness built from scratch with an outstanding performance/cost ratio, **something new
   that has not been made before**.
2. Tuned for the models he mainly uses: **DeepSeek V4.1 Flash and GLM-5.3** (cheap, think for long).
3. **A desktop app**, with a simple but modern UI.
4. **Rust for everything possible.**
5. Live testing through **logfare.ai**. It logs every prompt: synthetic tasks only.
6. Subagents routed to `claude-fable-5-2`. The delegation tool takes a model id only; effort and
   "fast mode" cannot be set.

He is terse, steers often, watches his usage (work in few turns) and wants candid status: report
numbers whichever way they fall. Ask before installing anything beyond Rust 1.99.1, the apt packages
in `scripts/setup-dev.sh` and the crates in `Cargo.lock`.

## Where everything is

- Repo `augult/harnessnew`, branch **`hoplite/naxos-31a5bb7b`**, **pull request #1 into `main`**
  (open, linked to the thread). `main` is the first handoff commit. GitHub's default branch is still
  `hoplite/axos-0f41d603`: that is a repository setting the agent's token cannot change.
- `crates/presage-core` (engine), `crates/presage-cli` (`presage` binary; `src/bench/` is the
  benchmark runner and `bench/<task>/{workspace,hidden,solution}` its tasks), `desktop/src-tauri` and
  `desktop/ui` (Tauri 2.12 shell, static front end), `.hoplite/setup.sh`, `scripts/setup-dev.sh`.
- `README.md` (design, status, headline numbers), `docs/BENCHMARKS.md` (every number and how to
  reproduce it), `docs/results/` (raw results), `docs/DESKTOP_UI_BRIEF.md` (UI specification),
  `docs/screenshots/`.
- Start reading at `crates/presage-core/src/lib.rs`, then `script.rs`, `agent.rs` (`opening`
  builds the first message), `watches.rs`, `attach.rs`, `harness.rs`.

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

- `cargo test`: **164 passing** (158 core, 6 CLI). `presage bench --check`: 7 of 7 tasks fail as
  shipped and pass with their reference solution.
- **Baseline benchmark** (engine before attach), 7 tasks × 5 runs × 4 variants per model, all 280
  runs solved. GLM-5.3: Presage 3 turns against 6 for classic at the median, cost $0.0077 against
  $0.0088 (−13%; −8% with no cache), total spend $0.469 against $0.864. DeepSeek V4.1 Flash: 3
  turns against 6, cost $0.0008 against $0.0009 at the median but 17% higher at the mean. Tables
  and a candid reading are in `docs/BENCHMARKS.md`.
- **DeepSeek answers through Logfare.** `deepseek-v4.1-flash-1017` is fast (about a second a turn,
  60–80% cached). `economy/deepseek-v4.1-flash` also answers but is slow (34–88 s for a small run),
  uncached and three times the price. Only the OpenRouter-shaped dialect is exercised; the direct
  DeepSeek dialect needs a DeepSeek key.
- **`attach` and `short_paths`** (written by a subagent, reviewed line by line, tests re-run by the
  parent): merged. Their live measurement was **not finished**; see the first open item.
- **Desktop UI** (commit `8a7eb23`, a subagent's second pass over the earlier draft): builds with no
  warnings. The parent took screenshots on the final build and looked at them: the walkthrough
  session (watch `tests` appearing, the amber surprise, the skipped `finish` with its reason, the
  verified result card, the inspector), the new-session form without a key, and settings
  (`docs/screenshots/`). The subagent reports that it also exercised, headless through scripted
  hooks: approval cards (Allow, Deny), Stop, the follow-up box, live streaming, the folder picker,
  `out:N`, saving and removing a key, defaults, and a replay of hand-written events of every type
  including an unknown one. The parent did not re-check those, and their screenshots and fixtures
  were in `/tmp` and are gone.
- **`.hoplite/setup.sh`** runs under the platform's setup (`sandbox_control` op `setup`: exit 0),
  in a sandbox that already had the packages. A cold sandbox is the remaining proof.

## What is not done, in priority order

1. **Measure `attach` properly.** The follow-up benchmark was still running when the session ended.
   Partial data points the right way: on GLM-5.3 after 55 of 140 runs the median was 2 turns with
   attach against 3.5 without and 6 for classic, with several one-turn runs. Check
   `docs/results/2026-11-11-*`: a non-empty `-attach.txt` means that run finished; otherwise repeat
   it (`--variants presage,no-attach,no-short-paths,classic -n 5 -j 3`, one model at a time) and
   update `docs/BENCHMARKS.md` and the README table.
2. **Cost.** Turns halve but cost does not follow, least of all on DeepSeek. Presage's turns are
   heavier: fixed prefix 1,193 estimated tokens against 762, preflight output in the first message,
   anchors on every line read. Candidates: cap the preflight output (the `tariff` check prints 140
   lines), drop `expand` windows for files that are attached anyway, a shorter system prompt.
3. **The `finish` turn.** The model sends `finish` in a reply of its own in most runs, which costs
   a full re-send of the context. Untried: a summary-carrying flag on `edit`/`write` that finishes
   if the watches pass; saying "you can finish now" in the receipt of a passing check.
4. **Desktop UI, what nobody has seen:** a session against a real provider started from the app,
   closing the window while a step runs, macOS and Windows, a release build, clippy. One deviation
   from the brief: login-shell PATH adoption is skipped when a standard stream is a terminal.
   Engine wishes from that work, none implemented: `Harness::events` drops lines it cannot type
   (the shell reads the log raw instead); no orderly shutdown (a command that ignores SIGTERM
   survives the app); `ProviderInfo` cannot say whether a key is stored or from the environment;
   `Approval` does not name its step; no event when a watch is switched off.
5. **The benchmark cannot show reliability.** Almost every run is solved (one classic run in the
   follow-up declared itself done and failed the held-out check). It needs harder tasks and more
   than 5 runs per cell; `roman` alone swings between 2 and 13 turns.
6. Direct DeepSeek, Z.ai and OpenRouter dialects are untested (no keys).
7. Smaller gaps: sessions cannot be resumed after an app restart; Windows shell handling; no OS
   sandbox for commands; effort ratchet and lesson recall are tested with the scripted model only.

## Facts established by live probing (do not re-derive)

- Logfare: OpenAI-compatible at `https://logfare.ai/v1`, bearer key, OpenRouter-shaped (`reasoning:
  {effort}` + `include_reasoning: true`; usage in a final chunk). Models that answer: `glm-5.3`,
  `deepseek-v4.1-flash-1017`, `economy/deepseek-v4.1-flash`.
- **The limit is 80 requests a minute for the whole account.** GLM-5.3 at `-j 3` stays under it.
  DeepSeek at `-j 3` goes over (HTTP 429 "request budget", retry in 25 s; the harness waits and
  retries). Run one benchmark at a time, DeepSeek with `-j 2`.
- A 140-run benchmark takes about 20 minutes and $2.50 on GLM-5.3 (a few `roman` runs think for
  minutes) and about 7 minutes and $0.16 on DeepSeek.
- GLM-5.3 returns several tool calls per reply. Its cache through the gateway is erratic (58–73%
  cached at the median with a byte-stable prefix), which is why the "no cache" column exists.
- Before attach a Presage run was `read → edit → finish` on both models. `finish` in the same reply
  as the edits: 6 of 35 runs on GLM-5.3, 1 of 35 on DeepSeek.
- With a watch in place the model makes almost no predictions of its own (11 to 14 in 35 runs,
  against about 50 without auto-watch).

## Sandbox notes that cost time

- The thread starts on its own branch. Fetch this one with an explicit refspec and check it out
  without `--track` (the clone is single-branch):
  `git fetch origin refs/heads/hoplite/naxos-31a5bb7b:refs/remotes/origin/hoplite/naxos-31a5bb7b`,
  then `git checkout -b hoplite/naxos-31a5bb7b origin/hoplite/naxos-31a5bb7b`.
- If `pkg-config --modversion webkit2gtk-4.1` fails, setup did not install the libraries; do it by
  hand as root:
  `apt-get update -qq && apt-get install -y -qq --no-install-recommends libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev pkg-config build-essential xvfb imagemagick`.
  `cargo test` does not build `target/debug/presage`; run `cargo build` first.
- Tool output over about 2,000 tokens is replaced by a lossy summary: read files in ranges of at
  most 100 lines and trim command output.
- The command policy blocks: piping into an interpreter, running or even syntax-checking a shell
  script you wrote (`bash x.sh`, `bash -n x.sh`), a variable as the command, heredocs or loops in a
  command that also calls `git` or `gh`. Write files with the file tool, then run plain commands.
  Push with an explicit refspec: `git push origin HEAD:refs/heads/hoplite/naxos-31a5bb7b`.
- Long runs: copy the binary first (`cp target/debug/presage /tmp/presage-bin/…`) so that a rebuild
  cannot change a benchmark under way, and start it with `nohup setsid … > report 2> progress &`.
  The one run started through the tool's own background mode died after two minutes.
- `/tmp` does not outlive the session. Commit results as soon as they exist.
- Subagents share the parent's files. Give a parallel engine task its own `git worktree` and take
  its work as a patch; build the desktop crate with `--target-dir target/desktop`. A subagent cannot
  be messaged or stopped, and it keeps running when the user interrupts the parent.
- The Logfare key is not in the repository and must never be. `LOGFARE_API_KEY` was not set as a
  project variable in this session; Lars pasted a key in chat instead (for the second time). It
  should be rotated and set as a project variable.

## Suggested first moves

1. Fetch the branch, `cargo test` (164 passing), `cargo build`.
2. Finish the `attach` measurement (item 1) and put the numbers in the README.
3. Go after cost and the `finish` turn (items 2 and 3), measuring each change with
   `presage bench --variants presage,no-<mechanism>,classic` on both models.
4. Run a real session from the desktop app.
