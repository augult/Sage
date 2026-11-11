# Handoff: Presage

Read this first if you are picking the project up. It is written for someone with no prior context
and replaces every earlier version. State: 11 November 2026, end of the third session.

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

- Repo `augult/harnessnew`, branch **`hoplite/naxos-31a5bb7b`**, with an open pull request into
  `main` (`gh pr list --head hoplite/naxos-31a5bb7b`). `main` is the first handoff commit. GitHub's
  default branch is still `hoplite/axos-0f41d603`: changing it is a repository setting the agent's
  token cannot touch, so Lars has to do it.
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
- **`attach` and `short_paths`** (this session, written by a subagent, reviewed line by line):
  tested, merged, and measured live in the follow-up section of `docs/BENCHMARKS.md`.
- **Desktop UI** (this session, a subagent's second pass over the earlier draft): builds without
  warnings on the final engine. Headless screenshots taken on that build and looked at: the
  walkthrough session from reply 1's check to the end (watch `tests` appearing, the amber surprise,
  the skipped `finish` with its reason, the verified result card, the follow-up box, the inspector),
  the new-session form without a key, and settings (`docs/screenshots/`). They match the brief.
- `.hoplite/setup.sh` exists and calls `scripts/setup-dev.sh --system-only`.

## What is not done, in priority order

1. **Desktop UI: nothing beyond screenshots is verified.** Nobody has clicked through it. Not
   checked: a live session started from the app, approval cards, the follow-up box, the folder
   picker, saving and removing a key, `out:N` links, the top of a long timeline. The subagent added
   test hooks for this (`PRESAGE_UI_SIZE`, `PRESAGE_UI_SCRIPT` to play UI actions, grammar in
   `desktop/ui/js/hooks.js`; `PRESAGE_UI_SCRIPTED=<file>` to start a session from scripted replies)
   and took 15 screenshots of further states, of which only two were looked at by the parent. It
   was interrupted and never delivered its report: read `desktop/` critically.
2. **Cost.** Turns halve but cost does not follow, least of all on DeepSeek. Presage's turns are
   heavier: fixed prefix 1,193 estimated tokens against 762, preflight output in the first message,
   anchors on every line read. Candidates: cap the preflight output (the `tariff` check prints 140
   lines), drop `expand` windows for files that are attached anyway, a shorter system prompt.
3. **The `finish` turn.** The model sends `finish` in a reply of its own in most runs. A separate
   reply costs a full re-send of the context. Untried: a `summary`-carrying flag on `edit`/`write`
   that finishes if the watches pass; saying "you can finish now" in the receipt of a passing check.
4. **The benchmark cannot show reliability.** Almost every run of every variant is solved. It needs
   harder tasks (several files, a misleading test, a long-running check) and more than 5 runs per
   cell; `roman` alone swings between 2 and 13 turns.
5. **`.hoplite/setup.sh` has not been run by the platform** (`sandbox_control` op `setup`). The
   command policy also refused `bash -n` on it, so it is unchecked beyond reading.
6. Direct DeepSeek, Z.ai and OpenRouter dialects are untested (no keys).
7. Smaller gaps: sessions cannot be resumed after an app restart; Windows shell handling; no OS
   sandbox for commands; effort ratchet and lesson recall are tested with the scripted model only.

## Facts established by live probing (do not re-derive)

- Logfare: OpenAI-compatible at `https://logfare.ai/v1`, bearer key, OpenRouter-shaped (`reasoning:
  {effort}` + `include_reasoning: true`; usage in a final chunk). Models that answer: `glm-5.3`,
  `deepseek-v4.1-flash-1017`, `economy/deepseek-v4.1-flash`.
- **The limit is 80 requests a minute for the whole account.** GLM-5.3 at `-j 3` stays under it.
  DeepSeek at `-j 3` goes over (HTTP 429 "request budget", retry in 25 s; the harness waits and
  retries). Do not run two benchmarks at once without lowering `-j`.
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
- Until `.hoplite/setup.sh` is proven, install the libraries by hand (as root):
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
- `/tmp` does not outlive the session. Results that matter go into `docs/results/`.
- Subagents share the parent's files. Give a parallel engine task its own `git worktree` and take
  its work as a patch; build the desktop crate with `--target-dir target/desktop`. A subagent that
  is interrupted reports nothing and cannot be messaged.
- The Logfare key is not in the repository and must never be. `LOGFARE_API_KEY` was not set as a
  project variable in this session; Lars pasted a key in chat instead (for the second time). It
  should be rotated and set as a project variable.

## Suggested first moves

1. Fetch the branch, install the libraries, `cargo test` (164 passing), `cargo build`.
2. Run the platform setup once to prove `.hoplite/setup.sh`.
3. Drive the desktop app through `PRESAGE_UI_SCRIPT` and a live session; fix what breaks.
4. Go after cost (item 2 above) and measure each change with
   `presage bench --variants presage,no-<mechanism>,classic` on both models.
