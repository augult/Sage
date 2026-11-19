# Sage

An AI coding agent harness for cheap reasoning models, with a CLI and a desktop app. Written in
Rust; the desktop app uses Tauri.

Before a command runs, the model says what it expects to happen. Sage checks that prediction
mechanically, so routine steps cost one line of output and the model's attention goes only where
reality surprised it. Several tool calls can go in one reply and run like a script that stops at
the first surprise, tests re-run on their own after every edit, and `finish` is accepted only once
a check has passed since the last change.

On a 36-task Python, Rust and Go benchmark with hidden tests (DeepSeek V4.1 Flash, 17 November
2026), Sage solved as many tasks as OpenCode, 44 of 72 each, at about 60% of its cost and time.
Details: [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md).

## Install

**Windows desktop app:** download `Sage_<version>_x64-setup.exe` from the
[latest release](https://github.com/augult/Sage/releases/latest) and run it. The standalone
`Sage_<version>_x64.exe` needs the Microsoft Edge WebView2 Runtime. The downloads are not
code-signed, so Windows may warn about an unknown publisher.

**From source** (CLI on any platform, desktop app on Linux, macOS or Windows): install Rust (the
version is pinned in `rust-toolchain.toml`; on Debian or Ubuntu, `scripts/setup-dev.sh` installs it
together with the libraries Tauri needs), then:

```sh
cargo build --release                                 # the CLI: target/release/sage
cargo run -p sage-desktop --target-dir target/desktop # the desktop app
cargo test                                            # the tests
```

## Use

Add an API key, in the desktop app's Settings or on the command line:

```sh
sage key deepseek            # reads the key from stdin; or export DEEPSEEK_API_KEY
sage demo                    # a scripted walkthrough, no key needed
sage run -C path/to/project "Fix the failing tests"
sage run --goal "the tests pass and the README names every flag" -C path/to/project
sage sessions                # list stored sessions; sage show <id> replays one
sage resume <id> "and now the other one"
```

`sage --help` lists every command and option.

- **Modes.** Sage (the default) is everything above. `--mode default` is the conventional loop
  other harnesses run, one tool call after another. Any single mechanism can be switched off with
  `--without <name>` or on with `--with <name>`.
- **Effort.** `-e ratchet` (Auto in the app, the default) starts at low and rises when surprises
  pile up; `low`, `high` and `max` stay where they are set.
- **Goals.** `--goal`, or `/goal <condition>` in a message, keeps the session working until a
  separate check agrees the condition holds.
- **Sessions survive restarts.** A session stored under `~/.sage` (or `SAGE_HOME`) can be continued
  after the app or the CLI was closed.
- **Skills.** `ui`, `security`, `cleanup`, `debug`, `tests` and `performance` come built in; a
  project's `.sage/skills` and `AGENTS.md` add to them.
- **Providers.** OpenAI, Anthropic, Google, DeepSeek, OpenRouter, Moonshot, xAI, Groq, Together,
  Logfare, and any OpenAI-compatible endpoint of your own. Only Logfare (DeepSeek V4.1 Flash and
  GLM-5.3) has been tested live.
- **Benchmarks.** `sage bench` runs Sage, or other harnesses such as OpenCode, on the same tasks and
  judges every run with hidden checks.

## More

- [`docs/DESIGN.md`](docs/DESIGN.md): how each mechanism works, sessions, goals, providers, releases.
- [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md): every measured number, with raw results.
- [`docs/HANDOFF.md`](docs/HANDOFF.md): what has been done and what is open.

```
crates/sage-core     the engine (no UI dependencies)
crates/sage-cli      the `sage` binary; bench/ holds the benchmark tasks
desktop/src-tauri    the Tauri shell
desktop/ui           the front end (static, no bundler)
docs                 design, benchmarks, handoff, UI brief, screenshots
```
