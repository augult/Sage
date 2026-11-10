# Presage

An expectation-driven agent harness, written in Rust: engine, CLI, and a Tauri desktop app.

Presage is built for cheap models that think for a long time (DeepSeek V4.1 Flash, GLM-5.3). On
those models a cached input token costs 1/50 of a fresh one and 1/200 of an output token, so the
scarce resource is not context. It is deliberation: every model turn is a fresh round of paid
thinking. And the usual failure is not lack of intelligence but an unnoticed mistake that compounds.

One primitive addresses both: **the model says what it expects, the harness checks it mechanically,
and the model's attention is spent only where reality diverged.**

## How it works

| Mechanism | What it does | Where |
|---|---|---|
| **A reply is a script** | All tool calls in one reply run in order. The first step that fails or contradicts its prediction halts the rest (`set -e` semantics). The model can commit to "edit, check, finish" in one round of thinking, safely. | `script.rs` |
| **Predictions and receipts** | `shell` takes `expect` (exit code, text that must / must not appear). A prediction that held is answered with a one-line receipt instead of the output; a violated one (a *surprise*) gets the output, the source lines it points at with edit anchors, and any lesson already learned about that error. Output size is proportional to surprise. | `guard.rs`, `tools/shell.rs`, `locate.rs` |
| **Watches** | Checks registered once and re-run by the harness after every change. They report only state changes: a regression is announced the moment it is introduced. | `watches.rs` |
| **Evidence gate** | `finish` is accepted only if a check has passed since the last change to the workspace (tracked with a cheap fingerprint). With fresh evidence it is accepted in the same reply as the check. | `state.rs`, `script.rs` |
| **Anchored edits** | Lines are shown as `41kq|…` (line number + 2-letter content hash). Edits address anchors, so the model never retypes old text, and a stale or shifted anchor is detected or relocated instead of corrupting the file. | `anchors.rs`, `tools/edit.rs` |
| **Reflex memory** | Error → fix pairs are learned mechanically per project (a command failed, a related one worked, no file was edited in between) and recalled at the next session. No model call involved. | `lessons.rs` |
| **Price-aware policy** | The harness knows the price vector (cache hit / miss / output). History is append-only and byte-stable so the provider cache always hits; every request is audited (harness bust vs provider miss); old observations are trimmed only when the arithmetic says it pays. | `economy.rs`, `context.rs` |
| **Effort ratchet** | Reasoning effort starts at `high` (below GLM-5.3's default `max`) and only rises, after repeated surprises. | `agent.rs` |
| **Robust plumbing** | Tool-call repair (malformed JSON, foreign argument names, calls written as text), thinking-loop watchdog, retries honouring `Retry-After`, commands that outlive their timeout become background jobs instead of being killed, noise-stripped output with the raw bytes kept on disk (`out:N`). | `repair.rs`, `provider/`, `tools/jobs.rs`, `text.rs` |

A `--classic` mode switches all of this off and runs a conventional tool loop with the same model
and tools, so every claim above can be measured rather than believed.

## Status

Honest state as of the last commit:

- **Engine and CLI**: implemented; `cargo test` runs 129 tests (unit tests per module plus
  end-to-end agent runs against a scripted model).
- **Walkthrough**: `presage demo` plays a three-turn scripted session through the real harness
  (reproduce, a wrong fix that is caught and halts the script, the right fix accepted with evidence).
- **Live models**: the wire format (streaming, multi-call replies carrying `expect`, reasoning
  replay, cache accounting) was verified with probes against GLM-5.3 on Logfare. See the commit
  history for whether full live task runs and A/B numbers have been added yet; until they are,
  **no performance or cost claim is made**.
- **Desktop app**: the Tauri shell builds and renders; the UI is in progress.
- Not implemented: resuming a session after restarting the app, Windows-specific shell handling,
  OS-level sandboxing of commands (there is a tripwire for catastrophic commands, nothing more).

## Build and run

Requires Rust 1.99.1 (pinned in `rust-toolchain.toml`). On Debian/Ubuntu, `scripts/setup-dev.sh`
installs the toolchain and the system libraries Tauri needs.

```sh
cargo test                      # engine + CLI tests
cargo build --release           # builds target/release/presage

presage demo                    # scripted walkthrough, no key needed
presage key logfare             # store an API key (read from stdin), or export LOGFARE_API_KEY
presage run -C path/to/project "Fix the failing tests"
presage run --classic ...       # baseline loop, for comparison
presage sessions | presage show <id>
```

Providers built in: `logfare`, `deepseek`, `zai`, `zai-coding`, `openrouter`, plus a custom
OpenAI-compatible endpoint in `~/.presage/config.json`. Only Logfare has been exercised live.

Desktop app:

```sh
cargo run -p presage-desktop --target-dir target/desktop
# installers: cargo install tauri-cli --version 2.12.4 --locked && cargo tauri build
```

## Layout

```
crates/presage-core   the engine (no UI dependencies)
crates/presage-cli    the `presage` binary
desktop/src-tauri     Tauri shell
desktop/ui            static front end (no bundler)
```

Sessions, spilled outputs, lessons and configuration live under `~/.presage` (`PRESAGE_HOME`).

## Background

The design draws on public work on harness engineering: Terminal-Bench harness comparisons
(Terminus-KIRA, Meta-Harness environment bootstrapping, AHE's ablations showing memory and tools
matter more than prompt wording), hash-anchored edit formats, and DeepSeek's and Z.ai's caching and
thinking contracts. The expectation mechanism (guarded script turns, surprise-proportional
observations, standing watches, evidence-linked finish) is this project's own contribution.
