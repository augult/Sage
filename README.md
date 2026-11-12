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
| **Predictions and receipts** | `shell` takes a prediction (`expect_exit`, `expect_has`, `expect_lacks`). A predicted success that held is answered with a one-line receipt instead of the output; a violated one (a *surprise*) gets the output, the source lines it points at with edit anchors, and any lesson already learned about that error. Output size is proportional to surprise. | `guard.rs`, `tools/shell.rs`, `locate.rs` |
| **Watches** | Checks registered once and re-run by the harness after every change. They report only state changes: a regression is announced the moment it is introduced. The first test command the model runs becomes a watch without being asked, so an edit-only reply comes back checked. | `watches.rs` |
| **Before the first turn** | What the harness can find out without the model is in the first message: a snapshot of the environment, the budget in replies, the files the task names (as `read` output, so the first reply can edit them), and the result of a test command the task names, which is watched from then on. A small workspace (up to 40 files) is surveyed: every file is listed with its length, and as much of it as fits in 1,000 lines is attached after the named files, so the model does not spend its first replies asking for files one at a time. | `bootstrap.rs`, `attach.rs`, `watches.rs` |
| **Evidence gate** | `finish` is accepted only if a check has passed since the last change to the workspace (tracked with a cheap fingerprint). With fresh evidence it is accepted in the same reply as the check. A check is any predicted command with a part that can fail (`cd dir && pytest` counts, `echo ok` does not). The gate cannot trap the model: refused twice for the same change, the third `finish` is accepted as unverified. Nor can the budget swallow the report: the last reply the budget allows is announced one reply ahead, and its `finish` is taken as it stands (no refusal, no review, even behind a failed step), marked verified or not by what is actually known. | `state.rs`, `script.rs`, `agent.rs` |
| **Review pass** | The first `finish` after a change is answered once, instead of being accepted, with the task quoted back, what the session changed (`git status` and the diff, without anything that was already uncommitted before it began) and a three-point review. The model reviews what is in front of it instead of spending a reply on fetching it. | `review.rs`, `script.rs` |
| **Skills** | Six hand-written instruction sets (`ui`, `security`, `cleanup`, `debug`, `tests`, `performance`), switched on per session or per message; project and user skills override the built-ins. | `skills.rs`, `skills/*.md` |
| **Editable system prompt** | The template is a file; a user copy replaces it, and a global and a project `AGENTS.md` are appended. `presage prompt` prints what the model gets. | `prompt.rs`, `prompts/system.md` |
| **Anchored edits** | Lines are shown as `41kq|…` (line number + 2-letter content hash). Edits address anchors, so the model never retypes old text. The harness keeps the line shifts of its own edits, so an anchor from before an edit (every anchor of a reply that edits the same file twice) is carried to where its line is now; one that fits nowhere, or in two places, is refused with the current lines. A two-letter hash is checked, never searched for. | `anchors.rs`, `tools/edit.rs` |
| **Reflex memory** | Error → fix pairs are learned mechanically per project (a command failed, a related one worked, no file was edited in between) and recalled at the next session. No model call involved. | `lessons.rs` |
| **Price-aware policy** | The harness knows the price vector (cache hit / miss / output). History is append-only and byte-stable so that the provider's cache can hit; every request is audited (harness bust vs provider miss); old observations are trimmed only when the arithmetic says it pays. | `economy.rs`, `context.rs` |
| **Effort ratchet** | Reasoning effort starts at `high` (below GLM-5.3's default `max`) and rises after repeated surprises. It falls only when a reply is nothing but reasoning: such a reply is cut at about 14,000 tokens of thinking (or at the output limit) and asked for again one level lower. | `agent.rs`, `provider/openai.rs` |
| **Robust plumbing** | Tool-call repair (malformed JSON, foreign argument names, calls written as text), thinking-loop watchdog, retries honouring `Retry-After` (without end while auto-retry is on, so a run outlasts a rate limit), commands that outlive their timeout become background jobs instead of being killed, noise-stripped output with the raw bytes kept on disk (`out:N`), command output shown with paths relative to the workspace. | `repair.rs`, `provider/`, `tools/jobs.rs`, `text.rs`, `workspace.rs` |

A `--classic` mode switches all of this off and runs a conventional tool loop with the same model
and tools, and `presage bench` runs both, or Presage with any one mechanism switched off, over a set
of tasks. Every claim above can be measured rather than believed.

## Status

State on 11 November 2026. What is open and how to continue: `docs/HANDOFF.md`. Every number:
`docs/BENCHMARKS.md`.

- **Engine and CLI**: implemented; `cargo test` runs 205 tests (unit tests per module plus
  end-to-end agent runs against a scripted model).
- **Not yet measured**: the changes of the seventh session (the last reply of the budget, the
  review that carries the diff, anchors carried through edits, the tool fixes listed in
  `docs/HANDOFF.md`) were made without a benchmark run. The numbers below are from before them.
- **Benchmark**: `presage bench` runs 24 synthetic tasks in three sets (`easy`, `skill`, `hard`)
  under harness variants, and other harnesses (`ext:mini`, `ext:opencode`, …) on the same tasks
  through a metering proxy. Every run is judged with held-out checks, whatever the agent claimed.
  On the skill and hard sets with DeepSeek V4.1 Flash (17 tasks, one run each): Presage 16/17 in
  14.3 turns and 128 s, classic 15/17 in 20.8 turns and 139 s, mini-swe-agent 16/17 in 13.9 turns
  and 165 s. GLM-5.3 was measured on six tasks only. See `docs/BENCHMARKS.md` for what limits these
  numbers.
- **Live models**: GLM-5.3 and DeepSeek V4.1 Flash both run end to end through Logfare. Every
  other preset (OpenAI, Anthropic, Google Gemini, xAI, Mistral, DeepSeek, Groq, Together,
  Fireworks, OpenRouter, Z.ai) follows its vendor's documented contract and is untested.
- **Desktop app**: built (`docs/screenshots/`). The Tauri shell has the commands the brief lists
  and the front end has the session list, the new-session form with a skills picker, the live
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
presage run --classic ...       # baseline loop, for comparison
presage sessions | presage show <id>

presage bench --check --tasks all   # the tasks fail as shipped and pass with their reference solutions
presage bench -p logfare -m glm-5.3 --variants presage,no-attach,classic -n 5 -j 3
```

Providers built in: `openai`, `anthropic`, `google`, `xai`, `mistral`, `deepseek`, `groq`,
`together`, `fireworks`, `openrouter`, `logfare`, `zai`, `zai-coding`, plus a custom
OpenAI-compatible endpoint in `~/.presage/config.json`. All of them are called through their
OpenAI-compatible chat-completions endpoints. Only Logfare has been exercised live: the others
are written from the vendors' documentation (`provider/openai.rs` has what each one takes), and
their model lists are what each vendor documented on 11 November 2026.

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

Sessions, spilled outputs, lessons and configuration live under `~/.presage` (`PRESAGE_HOME`).

## Background

The design draws on public work on harness engineering: Terminal-Bench harness comparisons
(Terminus-KIRA, Meta-Harness environment bootstrapping, AHE's ablations showing memory and tools
matter more than prompt wording), hash-anchored edit formats, and DeepSeek's and Z.ai's caching and
thinking contracts. The expectation mechanism (guarded script turns, surprise-proportional
observations, standing watches, evidence-linked finish) is this project's own contribution.
