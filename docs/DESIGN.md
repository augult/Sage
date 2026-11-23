# Sage: design and details

The long form of the README: how each mechanism works, the modes, the session goal, continuing a
session, providers, releases and the state of the project. The benchmark results are in
`docs/BENCHMARKS.md`.

## How it works

| Mechanism | What it does | Where |
|---|---|---|
| **A reply is a script** | All tool calls in one reply run in order. The first step that fails or contradicts its prediction halts the rest (`set -e` semantics). The model can commit to "edit, check, finish" in one round of thinking, safely. | `script.rs` |
| **Predictions and receipts** | `shell` takes a prediction (`expect_exit`, `expect_has`, `expect_lacks`). A predicted success that held is answered with a one-line receipt instead of the output; a violated one (a *surprise*) gets the output, the source lines it points at with edit anchors. Output size is proportional to surprise. | `guard.rs`, `tools/shell.rs`, `locate.rs` |
| **Watches** | Checks registered once and re-run by the harness after every change. They report only state changes: a regression is announced the moment it is introduced. The first test command the model runs becomes a watch without being asked, so an edit-only reply comes back checked. A test command the task itself names stays the watch whatever else the model runs, and a throwaway script (one outside the workspace, or written on the spot) never becomes one. | `watches.rs` |
| **Before the first turn** | What the harness can find out without the model is in the first message: a snapshot of the environment, the budget in replies, the files the task names (as `read` output, so the first reply can edit them), and the result of a test command the task names, which is watched from then on. A small workspace (up to 40 files) is surveyed: every file is listed with its length, and as much of it as fits in 1,000 lines is attached after the named files, so the model does not spend its first replies asking for files one at a time. | `bootstrap.rs`, `attach.rs`, `watches.rs` |
| **Evidence gate & Guard files** | `finish` is accepted only if a check has passed since the last change to the workspace (tracked with a cheap fingerprint of every file that is not ignored, so build output and caches are no change; in a tree too large to walk, git's own status covers it), With `protect`, the "Guard files" setting (off by default), files the task forbids touching (`Do not edit …`, `Do not change any code`) are content-hashed at turn zero and must be unchanged too: it reads the task's words by pattern and can hold correct work, and no model of the stored benchmark runs broke such a rule. Files a task asks to write are the model's to write: a check that they exist was taken out, as it read the task by pattern too and no run of the current mode lost a task that way. With fresh evidence `finish` is accepted in the same reply as the check. Neither gate can trap the model, and the last reply the budget allows is announced one reply ahead with `finish` taken as it stands. | `state.rs`, `contract.rs`, `script.rs`, `agent.rs` |
| **Goal** (`/goal`) | The user states a completion condition and the session keeps working until an independent check agrees it holds, as with `/goal` in Claude Code and Codex CLI. Where a run would end (`finish` accepted, or an answer in prose), one separate request to the same model, with a fresh context, no tools and the least thinking, is shown the goal, what the model said, what the harness knows about verification and what the session changed (`git status` and the diff, without anything that was already uncommitted before it began). `NOT MET` sends the model back with the reasons, at most three checks per message; `MET` is recorded in the session. See below. | `goal.rs`, `changes.rs`, `agent.rs` |
| **Skills** | Six hand-written instruction sets (`ui`, `security`, `cleanup`, `debug`, `tests`, `performance`), switched on per session or per message; project and user skills override the built-ins. | `skills.rs`, `skills/*.md` |
| **Editable system prompt** | The template is a file; a user copy replaces it, and a global and a project `AGENTS.md` are appended, each behind a fixed line saying that it is standing instructions and not a message in the conversation. `sage prompt` prints what the model gets. | `prompt.rs`, `prompts/system.md` |
| **Anchored edits** | Lines are shown as `41kq|…` (line number + 2-letter content hash). Edits address anchors, so the model never retypes old text. The harness keeps the line shifts of its own edits, so an anchor from before an edit (every anchor of a reply that edits the same file twice) is carried to where its line is now; one that fits nowhere, or in two places, is refused with the current lines. A two-letter hash is checked, never searched for. | `anchors.rs`, `tools/edit.rs` |
| **Price-aware policy** | The harness knows the price vector (cache hit / miss / output). History is append-only and byte-stable so that the provider's cache can hit; every request is audited (harness bust vs provider miss); old observations are trimmed only when the arithmetic says it pays. | `economy.rs`, `context.rs` |
| **Effort ratchet** | Reasoning effort starts at `low` and rises one level after three surprises in a row (`low`, `high`, `max`). It falls only when a reply is nothing but reasoning: such a reply is cut at about 14,000 tokens of thinking (or at the output limit) and asked for again one level lower, though thinking is switched off only at the second such reply of a run. A level set by hand (`low`, `high`, `max`) is left only for that one retry and is back once the model has acted; at `max` the cut comes at about 24,000 tokens, in every mode. After the third such cut in a row the next reply is the run's last, with `finish` taken as it stands. Starting at `high` (`-e ratchet-high`, the default until 12 November 2026) solved as many tasks and took about 30% more replies and twice the time on DeepSeek V4.1 Flash, and twice the replies on six tasks with GLM-5.3. | `agent.rs`, `provider/openai.rs` |
| **Robust plumbing** | Tool-call repair (malformed JSON, foreign argument names, calls written as text), thinking-loop watchdog, retries honouring `Retry-After` (without end while auto-retry is on, so a run outlasts a rate limit), commands that outlive their timeout become background jobs instead of being killed, a time budget that holds inside a reply, a command, a watch and a wait as well as between them, noise-stripped output with the raw bytes kept on disk (`out:N`), command output shown with paths relative to the workspace. | `repair.rs`, `provider/`, `tools/jobs.rs`, `text.rs`, `workspace.rs` |

`sage bench` runs any of the modes below, or one of them with a single mechanism switched on
or off, over a set of tasks. Every claim above can be measured rather than believed.

## Modes

A session runs in one of two modes (`--mode` on the CLI, a developer option that `sage --help`
does not show; the desktop app always uses Sage). The mode is fixed when
the session starts: its system prompt and its tools are sent with the first request.

| Mode | What it is for | What it does |
|---|---|---|
| **Sage** (`--mode sage`; the one a session gets unless told otherwise) | Few requests and little money, at a little cost to the result. | Everything in the table above. |
| **Default** (`--mode default`; `classic` in earlier results) | The loop every other harness runs, for those who want the app and not Sage's ways. | One tool call after another: no scripts, predictions, watches, finish gate or first-message survey. |

Any mechanism in the table above can be switched off in Sage (`--without <name>`,
`sage+no-<name>` in the bench) or on in the conventional loop (`--with <name>`,
`default+<name>`).

Earlier versions had a third mode, Prosage, with blind examiners and referees beside the run. It
was taken out because it solved fewer tasks than Sage at more than twice the cost. A session stored
as Prosage comes back as Sage.

**Effort is not part of the mode.** Every mode runs at the effort it is given. `ratchet` (Auto in
the app) starts at `low` and rises on surprises; `low`, `high` and `max` are kept as they are set.
At `max` a reply may think for about 24,000 tokens. A reply that does nothing but reason is still
asked for again one level lower, but under a level that was set the session is back at it as soon
as the model has acted.

## The session goal

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

## Continuing a session

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

## Providers

Providers built in: `openai`, `anthropic`, `google`, `deepseek`, `openrouter`, `moonshot`, `xai`,
`groq`, `together`, `logfare`, plus endpoints of your own: any number added in the desktop app's
Settings, or one written by hand as `custom` in `~/.sage/config.json`. All of them are called through their
OpenAI-compatible chat-completions endpoints. Only Logfare has been exercised live: the others
are written from the vendors' documentation (`provider/openai.rs` has what each one takes), and
their model lists are what each vendor documented on 11 November 2026 (Moonshot's, a day later).

## Developer commands

Some of the CLI is for working on Sage, not for using it: it works as ever but is left out of
`sage --help`, the `/` menu and `/help`, so users are not shown it.

| Command | What it is for |
|---|---|
| `sage bench` | Runs the benchmark tasks under harness variants and other harnesses (`sage bench --help`; `docs/BENCHMARKS.md`). `sage bench --scripted` is also the release's smoke test. |
| `--mode sage\|default` | For `run`, `chat` and `prompt`: Sage's loop or the conventional one (Modes, above). |
| `--with <mechanism>`, `--without <mechanism>` | For `run`, `chat` and `prompt`: one mechanism on or off, for ablation runs. |
| `/mode sage\|default` | In the interactive CLI: the mode of the sessions started after it. |
| `SAGE_UI_*` | The desktop app's test hooks (`desktop/ui/js/hooks.js`), `SAGE_UI_AUTODEMO` among them. |

## Releases

The **Release desktop** GitHub Actions workflow (`release.yml`) builds everything a release ships:
the Windows desktop app (installer and standalone executable, after the workspace's typecheck,
Clippy and tests) and the CLI for Linux x64, macOS on Apple silicon and Windows x64 (each after the
core and CLI tests; on Linux and macOS it then solves the seven `easy` benchmark tasks offline, a
scripted stand-in playing the model). Dispatched on a development branch it is a
preflight: the files are kept as workflow artifacts and nothing is published. Dispatched on `main`
it publishes the versioned GitHub release, with notes and SHA-256 checksums, once every job has
passed. Existing release tags are never overwritten.

A new release: raise `version` in `Cargo.toml` and in `desktop/src-tauri/tauri.conf.json`, rewrite
the notes in the workflow's publish job, push to `main`, and dispatch the workflow there. The
website's download button (augult/Harness) asks GitHub for the latest release, so it needs no change
unless the files are renamed.

## Status (23 November 2026, version 0.4.0)

- **Engine and CLI**: implemented; `cargo test --workspace` runs the unit tests of every module
  and end-to-end agent runs against a scripted model. CI runs them, Clippy with warnings as
  errors, formatting and the front end's lint on every pull request and push to `main`, on Linux
  and Windows, and the engine's and CLI's on macOS. `sage` in a terminal is an interactive session
  with `/` commands (`crates/sage-cli/src/chat.rs`); `sage run` is the unattended one.
- **Benchmark**: `sage bench` runs 24 synthetic tasks in three sets (`easy`, `skill`, `hard`)
  under harness variants, and other harnesses (`ext:mini`, `ext:opencode`, …) on the same tasks
  through a metering proxy. Every run is judged with held-out checks, whatever the agent claimed.
  The latest comparison, 22 November 2026, is in `docs/BENCHMARKS.md`: Sage solved more than
  OpenCode with DeepSeek V4.1 Flash and GPT-6 Luna, at lower cost and in less time.
- **Live models**: DeepSeek V4.1 Flash, GPT-6 Luna and GLM-5.3 have run end to end. Every other preset follows its vendor's documented contract and is untested.
- **Releases**: the CLI for Linux, macOS and Windows and the desktop app for Windows, on GitHub
  Releases. The repository is public under the MIT licence and shared as it is: issues and pull
  requests are open to collaborators only.
- **Desktop app**: Windows builds are published on GitHub Releases. It also builds on Linux and macOS. Its UI is
  covered by a browser fixture with mock IPC (`desktop/tests/`).
- **Walkthrough**: a scripted session through the real harness (`Harness::demo`), kept for tests and
  the desktop app's `SAGE_UI_AUTODEMO` screenshot hook. No command or menu offers it to users.
- Not implemented: OS-level sandboxing of commands (there is a tripwire for catastrophic commands,
  nothing more).

## Background

The design draws on public work on harness engineering: Terminal-Bench harness comparisons
(Terminus-KIRA, Meta-Harness environment bootstrapping, AHE's ablations showing memory and tools
matter more than prompt wording), hash-anchored edit formats, and DeepSeek's and Z.ai's caching and
thinking contracts. The expectation mechanism (guarded script turns, surprise-proportional
observations, standing watches, evidence-linked finish) is this project's own contribution.
