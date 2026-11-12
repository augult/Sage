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
| **Predictions and receipts** | `shell` takes a prediction (`expect_exit`, `expect_has`, `expect_lacks`). A predicted success that held is answered with a one-line receipt instead of the output; a violated one (a *surprise*) gets the output and the source lines it points at, with edit anchors. Output size is proportional to surprise. | `guard.rs`, `tools/shell.rs`, `locate.rs` |
| **Watches** | Checks registered once and re-run by the harness after every change. They report only state changes: a regression is announced the moment it is introduced. The first test command the model runs becomes a watch without being asked, so an edit-only reply comes back checked. A test command the task itself names stays the watch whatever else the model runs, and a throwaway script (one outside the workspace, or written on the spot) never becomes one. | `watches.rs` |
| **Before the first turn** | What the harness can find out without the model is in the first message: a snapshot of the environment, the budget in replies, the files the task names (as `read` output, so the first reply can edit them), and the result of a test command the task names, which is watched from then on. A small workspace (up to 40 files) is surveyed: every file is listed with its length, and as much of it as fits in 1,000 lines is attached after the named files, so the model does not spend its first replies asking for files one at a time. | `bootstrap.rs`, `attach.rs`, `watches.rs` |
| **Evidence gate** | `finish` is accepted only if a check has passed since the last change to the workspace (tracked with a cheap fingerprint). With fresh evidence it is accepted in the same reply as the check. A check is any predicted command with a part that can fail (`cd dir && pytest` counts, `echo ok` does not). The gate cannot trap the model: refused twice for the same change, the third `finish` is accepted as unverified. Nor can the budget swallow the report: the last reply the budget allows is announced one reply ahead, and its `finish` is taken as it stands (no refusal and no goal check, even behind a failed step), marked verified or not by what is actually known. | `state.rs`, `script.rs`, `agent.rs` |
| **Goal** (`/goal <condition>`, or `presage run --goal`) | The user states a completion condition, and the session works until a check agrees that it holds. Every `finish` the evidence gate lets through is put to one request of its own on the session's model: a fresh context (none of the conversation), no tools, the lowest effort the provider takes. It is given the goal, the `finish` message, the checks run since the workspace last changed with how each came out, and what the session changed (`git status` and a bounded diff, without anything that was already uncommitted before it began), and answers `MET` or `NOT MET: <what is missing>`. Not met, the `finish` is refused with that reason and the model goes on. The check cannot trap a session: an answer that does not come or cannot be read lets the `finish` through unchecked, a `finish` that comes back three times in a row with no change and no check in between is let through with the goal still open, and the reply and time budgets end the run as always. `/goal` alone says what the goal is and `/goal clear` removes it, neither with a request to the model. The goal is kept with the session until it is met or cleared, and it holds in `--classic` too. The check is the first of the finish checks, a list that anything else with a say on `finish` joins. | `goal.rs`, `finish.rs`, `changes.rs`, `script.rs` |
| **Skills** | Six hand-written instruction sets (`ui`, `security`, `cleanup`, `debug`, `tests`, `performance`), switched on per session or per message; project and user skills override the built-ins. | `skills.rs`, `skills/*.md` |
| **Editable system prompt** | The template is a file; a user copy replaces it, and a global and a project `AGENTS.md` are appended, each behind a fixed line saying that it is standing instructions and not a message in the conversation. `presage prompt` prints what the model gets. | `prompt.rs`, `prompts/system.md` |
| **Anchored edits** | Lines are shown as `41kq|…` (line number + 2-letter content hash). Edits address anchors, so the model never retypes old text. The harness keeps the line shifts of its own edits, so an anchor from before an edit (every anchor of a reply that edits the same file twice) is carried to where its line is now; one that fits nowhere, or in two places, is refused with the current lines. A two-letter hash is checked, never searched for. | `anchors.rs`, `tools/edit.rs` |
| **Price-aware policy** | The harness knows the price vector (cache hit / miss / output). History is append-only and byte-stable so that the provider's cache can hit; every request is audited (harness bust vs provider miss); old observations are trimmed only when the arithmetic says it pays. | `economy.rs`, `context.rs` |
| **Effort ratchet** | Reasoning effort starts at `low` and rises one level after three surprises in a row (`low`, `high`, `max`). It falls only when a reply is nothing but reasoning: such a reply is cut at about 14,000 tokens of thinking (or at the output limit) and asked for again one level lower, though thinking is switched off only at the second such reply of a run. Starting at `high` (`-e ratchet-high`, the default until 12 November 2026) solved as many tasks and took about 30% more replies and twice the time on DeepSeek V4.1 Flash, and twice the replies on six tasks with GLM-5.3. | `agent.rs`, `provider/openai.rs` |
| **Robust plumbing** | Tool-call repair (malformed JSON, foreign argument names, calls written as text), thinking-loop watchdog, retries honouring `Retry-After` (without end while auto-retry is on, so a run outlasts a rate limit), commands that outlive their timeout become background jobs instead of being killed, noise-stripped output with the raw bytes kept on disk (`out:N`), command output shown with paths relative to the workspace. | `repair.rs`, `provider/`, `tools/jobs.rs`, `text.rs`, `workspace.rs` |

A `--classic` mode switches all of this off but the goal, and runs a conventional tool loop with the
same model and tools, and `presage bench` runs both, or Presage with any one mechanism switched off,
over a set of tasks. Every claim above can be measured rather than believed.

## Status

State on 12 November 2026. What is open and how to continue: `docs/HANDOFF.md`. Every number:
`docs/BENCHMARKS.md`.

- **Engine and CLI**: implemented; `cargo test --workspace` runs 232 tests (unit tests per module plus
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
- **Goal** (`/goal`): tested against the scripted model only: the engine end to end, the desktop
  app under xvfb, and of the CLI its options and what it prints. No live model has answered a
  goal check yet and no benchmark has run with a goal, so what it costs and whether it helps is
  not known.
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
- Not implemented: resuming a session after restarting the app, Windows-specific shell handling,
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
presage run --goal "every test in tests/ passes"   # work until a check agrees; a task may follow
presage run --classic ...       # baseline loop, for comparison
presage sessions | presage show <id>

presage bench --check --tasks all   # the tasks fail as shipped and pass with their reference solutions
presage bench -p logfare -m glm-5.3 --variants presage,no-attach,classic -n 5 -j 3
```

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
