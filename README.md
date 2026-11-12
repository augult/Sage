# Presage

An expectation-driven agent harness, written in Rust: engine, CLI, and a Tauri desktop app.

Presage is built for cheap models that think for a long time (DeepSeek V4.1 Flash, GLM-5.3). On
DeepSeek a cached input token costs 1/50 of a fresh one and 1/200 of an output token (on GLM-5.3
about 1/5 and 1/17), so the scarce resource is not context. It is deliberation: every model turn is
a fresh round of paid thinking. And the usual failure is not lack of intelligence but an unnoticed
mistake that compounds.

One primitive addresses both: **the model says what it expects, the harness checks it mechanically,
and the model's attention is spent only where reality diverged.**

## How it works

| Mechanism | What it does | Where |
|---|---|---|
| **A reply is a script** | All tool calls in one reply run in order. The first step that fails or contradicts its prediction halts the rest (`set -e` semantics). The model can commit to "edit, check, finish" in one round of thinking, safely. | `script.rs` |
| **Predictions and receipts** | `shell` takes a prediction (`expect_exit`, `expect_has`, `expect_lacks`). A predicted success that held is answered with a one-line receipt instead of the output; a violated one (a *surprise*) gets the output, the source lines it points at with edit anchors. Output size is proportional to surprise. | `guard.rs`, `tools/shell.rs`, `locate.rs` |
| **Watches** | Checks registered once and re-run by the harness after every change. They report only state changes: a regression is announced the moment it is introduced. The first test command the model runs becomes a watch without being asked, so an edit-only reply comes back checked. A test command the task itself names stays the watch whatever else the model runs, and a throwaway script (one outside the workspace, or written on the spot) never becomes one. | `watches.rs` |
| **Before the first turn** | What the harness can find out without the model is in the first message: a snapshot of the environment, the budget in replies, the files the task names (as `read` output, so the first reply can edit them), and the result of a test command the task names, which is watched from then on. A small workspace (up to 40 files) is surveyed: every file is listed with its length, and as much of it as fits in 1,000 lines is attached after the named files, so the model does not spend its first replies asking for files one at a time. | `bootstrap.rs`, `attach.rs`, `watches.rs` |
| **Evidence gate** | `finish` is accepted only if a check has passed since the last change to the workspace (tracked with a cheap fingerprint). With fresh evidence it is accepted in the same reply as the check. A check is any predicted command with a part that can fail (`cd dir && pytest` counts, `echo ok` does not). The gate cannot trap the model: refused twice for the same change, the third `finish` is accepted as unverified. Nor can the budget swallow the report: the last reply the budget allows is announced one reply ahead, and its `finish` is taken as it stands (no refusal, even behind a failed step), marked verified or not by what is actually known. | `state.rs`, `script.rs`, `agent.rs` |
| **Goal** (`/goal`) | The user states a completion condition and the session keeps working until an independent check agrees it holds, as with `/goal` in Claude Code and Codex CLI. Where a run would end (`finish` accepted, or an answer in prose), one separate request to the same model, with a fresh context, no tools and the least thinking, is shown the goal, what the model said, what the harness knows about verification and what the session changed (`git status` and the diff, without anything that was already uncommitted before it began). `NOT MET` sends the model back with the reasons, at most three checks per message; `MET` is recorded in the session. See below. | `goal.rs`, `changes.rs`, `agent.rs` |
| **Skills** | Six hand-written instruction sets (`ui`, `security`, `cleanup`, `debug`, `tests`, `performance`), switched on per session or per message; project and user skills override the built-ins. | `skills.rs`, `skills/*.md` |
| **Editable system prompt** | The template is a file; a user copy replaces it, and a global and a project `AGENTS.md` are appended, each behind a fixed line saying that it is standing instructions and not a message in the conversation. `presage prompt` prints what the model gets. | `prompt.rs`, `prompts/system.md` |
| **Anchored edits** | Lines are shown as `41kq|…` (line number + 2-letter content hash). Edits address anchors, so the model never retypes old text. The harness keeps the line shifts of its own edits, so an anchor from before an edit (every anchor of a reply that edits the same file twice) is carried to where its line is now; one that fits nowhere, or in two places, is refused with the current lines. A two-letter hash is checked, never searched for. | `anchors.rs`, `tools/edit.rs` |
| **Price-aware policy** | The harness knows the price vector (cache hit / miss / output). History is append-only and byte-stable so that the provider's cache can hit; every request is audited (harness bust vs provider miss); old observations are trimmed only when the arithmetic says it pays. | `economy.rs`, `context.rs` |
| **Effort ratchet** | Reasoning effort starts at `low` and rises one level after three surprises in a row (`low`, `high`, `max`). It falls only when a reply is nothing but reasoning: such a reply is cut at about 14,000 tokens of thinking (or at the output limit) and asked for again one level lower, though thinking is switched off only at the second such reply of a run. Starting at `high` (`-e ratchet-high`, the default until 12 November 2026) solved as many tasks and took about 30% more replies and twice the time on DeepSeek V4.1 Flash, and twice the replies on six tasks with GLM-5.3. | `agent.rs`, `provider/openai.rs` |
| **Robust plumbing** | Tool-call repair (malformed JSON, foreign argument names, calls written as text), thinking-loop watchdog, retries honouring `Retry-After` (without end while auto-retry is on, so a run outlasts a rate limit), commands that outlive their timeout become background jobs instead of being killed, noise-stripped output with the raw bytes kept on disk (`out:N`), command output shown with paths relative to the workspace. | `repair.rs`, `provider/`, `tools/jobs.rs`, `text.rs`, `workspace.rs` |

`presage bench` runs any of the modes below, or one of them with a single mechanism switched on
or off, over a set of tasks. Every claim above can be measured rather than believed.

## Modes

A session runs in one of three modes (`--mode`, the Mode picker of the app). The mode is fixed when
the session starts: its system prompt and its tools are sent with the first request.

| Mode | What it is for | What it does |
|---|---|---|
| **Presage** (the one a session gets unless told otherwise) | Few requests and little money, at a little cost to the result. | Everything in the table above. |
| **Default** (`--mode default`; `classic` in earlier results) | The loop every other harness runs, for those who want the app and not Presage's ways. | One tool call after another: no scripts, predictions, watches, finish gate or first-message survey. |
| **Prosage** (`--mode prosage`) | The result, with requests and time spent freely. | Presage, without what it does to stay cheap, and with a second reader. See below. |

### What Prosage changes

Over the stored benchmark runs (`docs/results/`), twelve of the thirteen runs Presage lost on
DeepSeek V4.1 Flash it had declared finished and verified: the work passed the checks the model
chose to run and failed ones it never thought of. The conventional loop did not make that mistake
in any of its 94 runs, at twice the requests (`docs/BENCHMARKS.md`). Prosage is built against that
one failure, with four mechanisms that can each be switched on in Presage (`--with <name>`,
`presage+<name>` in the bench) or off in Prosage (`--without <name>`, `prosage+no-<name>`).
**It has not been measured against a live model yet**: the gateway was down for the rest of the
session that built it (`docs/HANDOFF.md` has the runs to make first).

| Mechanism | What it does | Where |
|---|---|---|
| **Examiner** (`examiner`) | A second reader. When the session starts, a separate request with a fresh context is given the task and the files the worker was shown, never the work, and writes one check script from the text alone: a case for every rule, example and error the task and its documents state. It is written beside the run, so it costs a request and no waiting. When `finish` is accepted the script is run against the work. Failing cases hold the `finish` and go back to the worker, which settles each by the text it cites: fix the work, or leave the case failing and say why. A `finish` whose failing cases the worker has already been shown stands. A script that does not run to its end, or fails more than three cases in five against work whose own checks pass, is set aside. In a session whose steps wait for approval (`ask`) there is no examiner: its script would be a command run unasked. | `examiner.rs`, `agent.rs` |
| **Long thought** (`long_thought`) | A reply may think for about 24,000 tokens before its first word or call, instead of being cut at 14,000 and told to act. On the specification-heavy task Presage loses most, three of five first replies were cut at that mark. | `provider/openai.rs` |
| **Unhurried** (`unhurried`) | Nothing the harness says counts replies or asks for an early `finish`: no budget line in the first message, no tips about saving replies, no half-time notes. When the checks pass, the model is told what passing checks leave open instead of that `finish` is accepted now. The limits themselves stay, as a safety net, with the note before the last reply. | `agent.rs` |
| **Routing** (`routing`) | One short request without thinking picks the skills whose subject is the task's main work, and they are switched on before the first word goes to the model. | `agent.rs`, `examiner.rs` |

The effort starts where Presage's does, at `low`: starting at `high` took twice the time and
solved no more tasks when it was measured on 12 November 2026. What Prosage adds is a second
reason for it to rise: a `finish` the examiner holds raises it one level. Requests made beside the
conversation (examiner, router) are counted in the session's tokens, cost and `side_requests`.

## Status

State on 12 November 2026. What is open and how to continue: `docs/HANDOFF.md`. Every number:
`docs/BENCHMARKS.md`.

- **Engine and CLI**: implemented; `cargo test` runs 253 tests (unit tests per module plus
  end-to-end agent runs against a scripted model).
- **Benchmark**: `presage bench` runs 24 synthetic tasks in three sets (`easy`, `skill`, `hard`)
  under harness variants, and other harnesses (`ext:mini`, `ext:opencode`, …) on the same tasks
  through a metering proxy. Every run is judged with held-out checks, whatever the agent claimed.
  Against OpenCode 1.19.5 with DeepSeek V4.1 Flash on 12 November 2026 (24 tasks, three runs
  each): Presage solved 67/72 in 5.7 requests and 59 s a run for $1.12, OpenCode 70/72 in 10.8
  requests and 116 s for $2.23. Presage loses two tasks that OpenCode solves every time
  (`restyle`, `confdialect`) and is the steadier on the other 22. The comparisons with classic and
  mini-swe-agent are older, from before the changes of the seventh and ninth sessions. See
  `docs/BENCHMARKS.md` for all of it and for what limits these numbers.
- **Live models**: GLM-5.3 and DeepSeek V4.1 Flash both run end to end through Logfare. Every
  other preset (OpenAI, Anthropic, Google AI, DeepSeek, OpenRouter, Moonshot Kimi, xAI, Groq,
  Together AI) follows its vendor's documented contract and is untested.
- **Desktop app**: built (`docs/screenshots/`). The Tauri shell has the commands the brief lists
  and the front end has the session list, the new-session form with a skills dialog, the live
  timeline with inspector, and settings for keys, defaults, skills, the system prompt and
  instructions. It builds without warnings and has been driven through scripted UI actions under
  xvfb, with screenshots: nobody has driven it by hand, and no live-model session has been started
  from it.
- **Walkthrough**: `presage demo` plays a three-reply scripted session through the real harness
  (read and reproduce, a wrong fix that is caught and halts the script, the right fix accepted with
  evidence).
- Not implemented: Windows-specific shell handling,
  OS-level sandboxing of commands (there is a tripwire for catastrophic commands, nothing more).

### What the benchmark shows

Logfare, 7 tasks, 5 runs per task and variant, medians per run; "no cache" is the cost had the
provider cached nothing. These rows are the baseline of 10 November, before `attach`.

| Model | Variant | Solved | Turns (mean) | Input tok | Output tok | Cost | No cache | Total spend | Time |
|---|---|---|---|---:|---:|---:|---:|---:|---:|
| GLM-5.3 | Presage | 35/35 | 3 (3.6) | 8.8k | 395 | $0.0077 | $0.0141 | $0.469 | 6.4 s |
| GLM-5.3 | Classic | 35/35 | 6 (7.4) | 9.9k | 415 | $0.0088 | $0.0154 | $0.864 | 9.8 s |
| DeepSeek V4.1 Flash | Presage | 35/35 | 3 (3.6) | 9.6k | 442 | $0.0008 | $0.0017 | $0.043 | 3.8 s |
| DeepSeek V4.1 Flash | Classic | 35/35 | 6 (5.6) | 10.4k | 639 | $0.0009 | $0.0019 | $0.037 | 5.4 s |

- **Turns halve on both models, and runs finish about a third sooner.** The saving comes from the
  harness running the check itself (preflight and auto-watch each account for one turn), not from
  the model's own predictions, of which it makes few once a watch exists.
- **Cost falls less.** On GLM-5.3 a typical run is about a tenth cheaper; the 46% gap in total
  spend is two runaway classic runs on one task. On DeepSeek there is no saving: three Presage
  turns carry about as many input tokens as six classic ones.
- **Reliability is not measured.** Every run of every variant was solved, so these tasks are too
  easy to separate the variants.
- **The model still spends a turn on `finish`.** It came in the same reply as the edits in 6 of 35
  runs on GLM-5.3 and 1 of 35 on DeepSeek. Before `attach`, almost every run was
  `read → edit → finish`.
- `attach` and `short_paths` were measured afterwards: see the follow-up in `docs/BENCHMARKS.md`.

## Build and run

Requires Rust 1.99.1 (pinned in `rust-toolchain.toml`). On Debian/Ubuntu, `scripts/setup-dev.sh`
installs the toolchain and the system libraries Tauri needs; `.hoplite/setup.sh` does the latter
when a Hoplite sandbox is set up.

```sh
cargo test                      # engine + CLI tests
cargo build --release           # builds target/release/presage

presage demo                    # scripted walkthrough, no key needed
presage key logfare             # store an API key (read from stdin), or export LOGFARE_API_KEY
presage run -C path/to/project "Fix the failing tests"
presage run --mode default ...  # the conventional loop; --mode prosage for the result at any price
presage run --goal "the tests pass and the README names every flag" -C path/to/project
presage sessions | presage show <id>
presage resume <id> "and now the other one"   # continue a stored session (see below)

presage bench --check --tasks all   # the tasks fail as shipped and pass with their reference solutions
presage bench -p logfare -m glm-5.3 --variants presage,no-attach,default,prosage -n 5 -j 3
```

### The session goal

A goal is a condition that has to hold before the work counts as done. State it with
`presage run --goal "<condition>" [task]` (with no task, the goal is the task), or as `/goal` at the
start of a task or of any follow-up message, in the CLI's task and in the desktop app's message box:

- `/goal <condition>` sets the goal and starts work towards it (the model is sent "Work until this holds: …").
- `/goal` says what the goal is, and `/goal clear` drops it. Neither makes a request to the model.

The goal belongs to the session and is kept with it. It stays active across follow-up messages until
a check finds it met (recorded as met, and it no longer gates what follows) or it is cleared. A run
that is about to end gets one check per attempt, at most three per message: a `NOT MET` answer sends
the model back with its reasons; after the third the run ends as usual, says in its evidence that the
goal is not met, and the goal stays active. A check that cannot be made or read never fails the run.
On the last reply the budget allows the check is still made so that the result is recorded, but
nothing can follow it. The checks are paid for like any request and show in the session's figures
(`goal_checks`). They work the same in every mode.

### Continuing a session

A session can be continued after the program that ran it has ended: the app was quit, the CLI was
killed, the machine went down. Open it in the desktop app and send a follow-up, or
`presage resume <id> [message]` (with no message the session is told to continue where it left off;
`-p`, `-m` and `-e` name another provider, model or effort for the message, as in the app). The model
sees the conversation it had, then the new message in front of which it is told that the session
was restored after a restart: background jobs and shell processes from before are gone, and the
files are as they are on disk now.

Each session keeps a `snapshot.json` beside `meta.json` and `events.jsonl`: the conversation as it
was sent (system prompt included), and what the agent would otherwise start empty with. That is the
mode and the mechanism lists, autonomy, limits, the effort policy and the level it had reached, the
turn counter and the stats, the files touched, what was uncommitted before the session began, the
standing watches (the checks, not their results: they run again) and the step and `out:N` counters.
It is rewritten whole (a temporary file, then a rename) after every model reply and when a run ends,
so a kill loses at most the turn in flight. A reply whose tool calls had no results when the process
died is made valid again by answering each call as interrupted. What is not kept starts empty: files
already read are shown again, anchors from before find their lines by content or the model reads
again, background jobs are gone, and the evidence of checks starts fresh. The goal is in `meta.json`.

The session goes on with the model it had, through the current configuration; where its key is gone,
the error says so. One process runs a session at a time: `session.lock` is an operating-system file
lock, so one left by a process that was killed is never in the way. A `resumed` event marks the
restoration in the log. Sessions stored before snapshots existed are read-only: their log holds no
system prompt, no composed first message and none of the harness's footers, so a conversation rebuilt
from it would not be the one the model had. Sessions a stand-in played are read-only too. `presage
sessions` and the app say which, in a word: `older`, `simulated`, `busy` (open in another process),
`gone` (its folder is).

Providers built in: `openai`, `anthropic`, `google`, `deepseek`, `openrouter`, `moonshot`, `xai`,
`groq`, `together`, `logfare`, plus endpoints of your own: any number added in the desktop app's
Settings, or one written by hand as `custom` in `~/.presage/config.json`. All of them are called through their
OpenAI-compatible chat-completions endpoints. Only Logfare has been exercised live: the others
are written from the vendors' documentation (`provider/openai.rs` has what each one takes), and
their model lists are what each vendor documented on 11 November 2026 (Moonshot's, a day later).

Desktop app:

```sh
cargo run -p presage-desktop --target-dir target/desktop
# installers: cargo install tauri-cli --version 2.12.4 --locked && cargo tauri build
```

## Layout

```
crates/presage-core   the engine (no UI dependencies)
crates/presage-cli    the `presage` binary; bench/ holds the benchmark tasks
desktop/src-tauri     Tauri shell
desktop/ui            static front end (no bundler)
docs                  handoff, benchmarks with raw results, UI brief, screenshots
```

Sessions, spilled outputs and configuration live under `~/.presage` (`PRESAGE_HOME`).

## Background

The design draws on public work on harness engineering: Terminal-Bench harness comparisons
(Terminus-KIRA, Meta-Harness environment bootstrapping, AHE's ablations showing memory and tools
matter more than prompt wording), hash-anchored edit formats, and DeepSeek's and Z.ai's caching and
thinking contracts. The expectation mechanism (guarded script turns, surprise-proportional
observations, standing watches, evidence-linked finish) is this project's own contribution.
