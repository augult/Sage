# Sage

An expectation-driven agent harness, written in Rust: engine, CLI, and a Tauri desktop app.

Sage is built for cheap models that think for a long time (DeepSeek V4.1 Flash, GLM-5.3). On
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
| **Editable system prompt** | The template is a file; a user copy replaces it, and a global and a project `AGENTS.md` are appended, each behind a fixed line saying that it is standing instructions and not a message in the conversation. `sage prompt` prints what the model gets. | `prompt.rs`, `prompts/system.md` |
| **Anchored edits** | Lines are shown as `41kq|…` (line number + 2-letter content hash). Edits address anchors, so the model never retypes old text. The harness keeps the line shifts of its own edits, so an anchor from before an edit (every anchor of a reply that edits the same file twice) is carried to where its line is now; one that fits nowhere, or in two places, is refused with the current lines. A two-letter hash is checked, never searched for. | `anchors.rs`, `tools/edit.rs` |
| **Price-aware policy** | The harness knows the price vector (cache hit / miss / output). History is append-only and byte-stable so that the provider's cache can hit; every request is audited (harness bust vs provider miss); old observations are trimmed only when the arithmetic says it pays. | `economy.rs`, `context.rs` |
| **Effort ratchet** | Reasoning effort starts at `low` and rises one level after three surprises in a row (`low`, `high`, `max`). It falls only when a reply is nothing but reasoning: such a reply is cut at about 14,000 tokens of thinking (or at the output limit) and asked for again one level lower, though thinking is switched off only at the second such reply of a run. A level set by hand (`low`, `high`, `max`) is left only for that one retry and is back once the model has acted; at `max` the cut comes at about 24,000 tokens, in every mode. Starting at `high` (`-e ratchet-high`, the default until 12 November 2026) solved as many tasks and took about 30% more replies and twice the time on DeepSeek V4.1 Flash, and twice the replies on six tasks with GLM-5.3. | `agent.rs`, `provider/openai.rs` |
| **Robust plumbing** | Tool-call repair (malformed JSON, foreign argument names, calls written as text), thinking-loop watchdog, retries honouring `Retry-After` (without end while auto-retry is on, so a run outlasts a rate limit), commands that outlive their timeout become background jobs instead of being killed, noise-stripped output with the raw bytes kept on disk (`out:N`), command output shown with paths relative to the workspace. | `repair.rs`, `provider/`, `tools/jobs.rs`, `text.rs`, `workspace.rs` |

`sage bench` runs any of the modes below, or one of them with a single mechanism switched on
or off, over a set of tasks. Every claim above can be measured rather than believed.

## Modes

A session runs in one of three modes (`--mode`, the Mode picker of the app). The mode is fixed when
the session starts: its system prompt and its tools are sent with the first request.

| Mode | What it is for | What it does |
|---|---|---|
| **Ecosage** (`--mode ecosage`; the one a session gets unless told otherwise) | Few requests and little money, at a little cost to the result. | Everything in the table above. |
| **Default** (`--mode default`; `classic` in earlier results) | The loop every other harness runs, for those who want the app and not Sage's ways. | One tool call after another: no scripts, predictions, watches, finish gate or first-message survey. |
| **Prosage** (`--mode prosage`) | The result, with requests and time spent freely. | Ecosage's loop without what it does to stay cheap, and with second readers: two examiners who never see the work, and referees who decide which of their failing cases the text bears out. See below. |

### What Prosage changes

Over the stored benchmark runs (`docs/results/`), twelve of the thirteen runs Sage lost on
DeepSeek V4.1 Flash it had declared finished and verified: the work passed the checks the model
chose to run and failed ones it never thought of. The conventional loop did not make that mistake
in any of its 94 runs, at twice the requests (`docs/BENCHMARKS.md`). Prosage is built against that
one failure. It is not Ecosage with the effort turned up: the effort is the same setting in every
mode (below), and what Prosage adds is done by the harness around the model. Its seven mechanisms
can each be switched on in Ecosage (`--with <name>`, `ecosage+<name>` in the bench) or off in
Prosage (`--without <name>`, `prosage+no-<name>`).
**Measured once, on 12 November 2026** (DeepSeek V4.1 Flash, 24 tasks, two runs each): Prosage
47/48, OpenCode 45/48, Ecosage 44/48, with Prosage at three times OpenCode's time and four times
its cost. The lead is within chance, and the examiners decided almost none of it:
`docs/BENCHMARKS.md` has the table and what the logs say, `docs/HANDOFF.md` what to run next,
`docs/RESEARCH.md` (section 3) what the design rests on.

| Mechanism | What it does | Where |
|---|---|---|
| **Examiner** (`examiner`) | A second reader. When the session starts, a separate request with a fresh context is given the task, the files the worker was shown and any longer or overflow files `attach` had to leave out (`<more_files>`), never the work, and writes one check script from the text alone: a case for every rule, example and error the task and its documents state. It is written beside the run, so it costs a request and no waiting. The worker is told with its task that the work will be examined this way. When `finish` is accepted (or when a watchless run checks its changes cleanly) the script is run against the work, refreshed from memory first. A script that does not run to its end or misses the header format goes back to its author with its output, twice at most; one more than three in five of whose cases fail against work whose own checks pass goes back once. What is still unusable after that is set aside. In a session whose steps wait for approval (`ask`) there is no examiner: its script would be a command run unasked. | `examiner.rs`, `attach.rs`, `agent.rs` |
| **Panel** (`panel`) | A second examiner, asked at the same moment and briefed for what a pass rule by rule leaves out: every error the text states and how it is reported, the limits of each rule, two rules meeting in one input. Where the text has nothing of that kind it answers NONE. Needs `examiner`. | `examiner.rs` |
| **Referee** (`referee`) | A failing case holds nothing by itself: a script written blind is wrong often enough to make a worker break work that was right. Each failing case goes to two referees, separate requests that are shown the text, the script and what the case reported, and not the work. Each works out from the text what the result has to be and rules GAP (quoting the sentence that settles it), CASE or OPEN. Two GAP rulings confirm a case at once; a split of one GAP against OPEN (with no CASE veto) goes to one tie-breaker referee, so a single hedged or truncated reply cannot drop a real gap while two independent GAP rulings are still required to hold. Confirmed cases reach the worker with what was quoted; the others are named as not to be acted on. Without referees (`prosage+no-referee`) every failing case goes to the worker. Needs `examiner`. | `examiner.rs`, `agent.rs` |
| **Contract** (`contract`) | Mechanical checks read off the task before the first turn: files and folders the task forbids changing (`Do not edit …`, `Do not change any code`) are content-hashed at turn zero, and deliverable files the task explicitly asks to write or create (`Write \`findings.json\``, `Create \`roman.py\``) must exist (and parse, for JSON) at the requested path before `finish` stands, even when `epoch == 0`. Costs no model requests. | `contract.rs`, `agent.rs` |
| **Long thought** (`long_thought`) | A reply may think for about 24,000 tokens before its first word or call, instead of being cut at 14,000 and told to act. On the specification-heavy task Sage loses most, three of five first replies were cut at that mark. | `provider/openai.rs` |
| **Unhurried** (`unhurried`) | Nothing the harness says counts replies or asks for an early `finish`: no budget line in the first message, no tips about saving replies, no half-time notes. When the checks pass (and no examiner report already speaks on that reply), the model is told what passing checks leave open instead of that `finish` is accepted now. An examiner still writing is waited for up to 15 minutes instead of 4. What takes the budget's place is a progress-aware stopping rule: consecutive surprise-free replies on the same settled workspace state are counted (resetting whenever a file changes or a test of the worker's own fails); after 8 the worker is told to finish or to name what is unmet, and after 16 its next reply is its last. The limits themselves stay, as a safety net, with the note before the last reply. | `agent.rs` |
| **Routing** (`routing`) | One short request without thinking picks the one skill whose subject is what the task asks to be delivered, if there is one, and it is switched on before the first word goes to the model. | `agent.rs`, `examiner.rs` |

The examiners' cases are run as soon as there is something to run them against: with the first
reply whose own checks pass on a change, and again at `finish` if the work has changed since.
Cases the referees confirm go to the worker then; when all pass it is told so and that it may
finish. (Without this, a worker in no hurry tested its own work for all its sixty replies on two
tasks and never called `finish`.)

A held `finish` goes back to the worker, which settles each case by the text: fix the work, or
leave the case failing and name in its summary the sentence that shows it wrong. A `finish` all of
whose failing cases the worker has been shown stands, and the outcome says how many were left.
A case that passed before the worker's last changes and fails after them is marked as broken by
them, so that a fix cannot quietly cost something that was right. Three holds at most per message.
With a time budget, nothing more is asked for beside the run once less than 45 seconds of it are
left.

**Effort is not part of the mode.** Every mode runs at the effort it is given. `ratchet` (Auto in
the app) starts at `low` and rises on surprises; `low`, `high` and `max` are kept as they are set,
in Ecosage as in Prosage. At `max` a reply may think for about 24,000 tokens in any mode. A reply
that does nothing but reason is still asked for again one level lower, but under a level that was
set the session is back at it as soon as the model has acted. Examiners and referees think at the
effort the session has reached, never below `high`. Under `ratchet`, a `finish` the examiners hold
raises the effort one level. Requests made beside the conversation (examiners, referees, router)
are counted in the session's tokens, cost and `side_requests`.

## Status

State on 12 November 2026. What is open and how to continue: `docs/HANDOFF.md`. Every number:
`docs/BENCHMARKS.md`.

- **Engine and CLI**: implemented; `cargo test` runs 253 tests (unit tests per module plus
  end-to-end agent runs against a scripted model).
- **Benchmark**: `sage bench` runs 24 synthetic tasks in three sets (`easy`, `skill`, `hard`)
  under harness variants, and other harnesses (`ext:mini`, `ext:opencode`, …) on the same tasks
  through a metering proxy. Every run is judged with held-out checks, whatever the agent claimed.
  Against OpenCode 1.19.5 with DeepSeek V4.1 Flash on 12 November 2026 (24 tasks, three runs
  each): Sage solved 67/72 in 5.7 requests and 59 s a run for $1.12, OpenCode 70/72 in 10.8
  requests and 116 s for $2.23. Sage loses two tasks that OpenCode solves every time
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
- **Walkthrough**: `sage demo` plays a three-reply scripted session through the real harness
  (read and reproduce, a wrong fix that is caught and halts the script, the right fix accepted with
  evidence).
- Not implemented: Windows-specific shell handling,
  OS-level sandboxing of commands (there is a tripwire for catastrophic commands, nothing more).

### What the benchmark shows

Logfare, 7 tasks, 5 runs per task and variant, medians per run; "no cache" is the cost had the
provider cached nothing. These rows are the baseline of 10 November, before `attach`.

| Model | Variant | Solved | Turns (mean) | Input tok | Output tok | Cost | No cache | Total spend | Time |
|---|---|---|---|---:|---:|---:|---:|---:|---:|
| GLM-5.3 | Ecosage | 35/35 | 3 (3.6) | 8.8k | 395 | $0.0077 | $0.0141 | $0.469 | 6.4 s |
| GLM-5.3 | Classic | 35/35 | 6 (7.4) | 9.9k | 415 | $0.0088 | $0.0154 | $0.864 | 9.8 s |
| DeepSeek V4.1 Flash | Ecosage | 35/35 | 3 (3.6) | 9.6k | 442 | $0.0008 | $0.0017 | $0.043 | 3.8 s |
| DeepSeek V4.1 Flash | Classic | 35/35 | 6 (5.6) | 10.4k | 639 | $0.0009 | $0.0019 | $0.037 | 5.4 s |

- **Turns halve on both models, and runs finish about a third sooner.** The saving comes from the
  harness running the check itself (preflight and auto-watch each account for one turn), not from
  the model's own predictions, of which it makes few once a watch exists.
- **Cost falls less.** On GLM-5.3 a typical run is about a tenth cheaper; the 46% gap in total
  spend is two runaway classic runs on one task. On DeepSeek there is no saving: three Sage
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
cargo build --release           # builds target/release/sage

sage demo                    # scripted walkthrough, no key needed
sage key logfare             # store an API key (read from stdin), or export LOGFARE_API_KEY
sage run -C path/to/project "Fix the failing tests"
sage run --mode default ...  # the conventional loop; --mode prosage for the result at any price
sage run --goal "the tests pass and the README names every flag" -C path/to/project
sage sessions | sage show <id>
sage resume <id> "and now the other one"   # continue a stored session (see below)

sage bench --check --tasks all   # the tasks fail as shipped and pass with their reference solutions
sage bench -p logfare -m glm-5.3 --variants ecosage,no-attach,default,prosage -n 5 -j 3
```

### The session goal

A goal is a condition that has to hold before the work counts as done. State it with
`sage run --goal "<condition>" [task]` (with no task, the goal is the task), or as `/goal` at the
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
`sage resume <id> [message]` (with no message the session is told to continue where it left off;
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
from it would not be the one the model had. Sessions a stand-in played are read-only too. `sage
sessions` and the app say which, in a word: `older`, `simulated`, `busy` (open in another process),
`gone` (its folder is).

Providers built in: `openai`, `anthropic`, `google`, `deepseek`, `openrouter`, `moonshot`, `xai`,
`groq`, `together`, `logfare`, plus endpoints of your own: any number added in the desktop app's
Settings, or one written by hand as `custom` in `~/.sage/config.json`. All of them are called through their
OpenAI-compatible chat-completions endpoints. Only Logfare has been exercised live: the others
are written from the vendors' documentation (`provider/openai.rs` has what each one takes), and
their model lists are what each vendor documented on 11 November 2026 (Moonshot's, a day later).

Desktop app:

```sh
cargo run -p sage-desktop --target-dir target/desktop
# installers: cargo install tauri-cli --version 2.12.4 --locked && cargo tauri build
```

## Layout

```
crates/sage-core   the engine (no UI dependencies)
crates/sage-cli    the `sage` binary; bench/ holds the benchmark tasks
desktop/src-tauri     Tauri shell
desktop/ui            static front end (no bundler)
docs                  handoff, benchmarks with raw results, UI brief, screenshots
```

Sessions, spilled outputs and configuration live under `~/.sage` (`SAGE_HOME`).

## Background

The design draws on public work on harness engineering: Terminal-Bench harness comparisons
(Terminus-KIRA, Meta-Harness environment bootstrapping, AHE's ablations showing memory and tools
matter more than prompt wording), hash-anchored edit formats, and DeepSeek's and Z.ai's caching and
thinking contracts. The expectation mechanism (guarded script turns, surprise-proportional
observations, standing watches, evidence-linked finish) is this project's own contribution.
