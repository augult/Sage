# Sage

A coding agent harness for cheap reasoning models: a terminal CLI and a desktop app, written in
Rust.

[Website](https://sageharness.vercel.app) · [Download](https://github.com/augult/Sage/releases/latest) ·
[How it works](docs/DESIGN.md) · [Benchmarks](docs/BENCHMARKS.md)

Before each command, the model says what it expects to happen, and Sage checks it mechanically. A
step that went as expected costs one line; only a surprise is shown in full, with the source lines
it points at. The model's attention goes where reality differed, and nowhere else.

- **A reply is a script.** The tool calls in one reply run in order and stop at the first surprise,
  so the model can edit, check and finish in one round of thinking.
- **Tests watch themselves.** The first test command the model runs is re-run after every edit, and
  the model hears about it only when the result changes.
- **Done means checked.** `finish` is accepted only once a check has passed since the last change.
- **The cache keeps hitting.** History is append-only, so the provider's prompt cache stays warm,
  and old output is trimmed only when that costs less than keeping it.

All of it serves one aim: the same work for less. Sage is built for cheap reasoning models and low
bills, with fewer requests, fewer tokens and less waiting than a conventional agent loop.

## Results

30 tasks, judged by tests the agent never saw: the 7 `hard` tasks built into `sage bench` and 23
Python and Rust exercises from the Exercism polyglot set, every task run twice per harness. Same
model, same tasks.

| Model | Harness | Solved | Cost at list price | Median time per run |
|---|---|---|---|---|
| DeepSeek V4.1 Flash | **Sage** | **45/60** | **$0.207** | **55 s** |
| | OpenCode 1.19.5 | 40/60 | $0.274 | 86 s |
| GPT-6 Luna | **Sage** | **30/60** | **$0.137** | **41 s** |
| | OpenCode 1.19.5 | 24/60 | $0.174 | 61 s |

Sage solved more with both models, for 21 to 24% less money and about a third less time. These
were run by me on 22 November 2026; gaps of two or three runs are within chance, and on your own
code the results can differ, in either direction, so the best test is your own work. Per-run data
is in [`docs/results/`](docs/results/), and the method in [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md).

## Install

From the [latest release](https://github.com/augult/Sage/releases/latest), or the
[website](https://sageharness.vercel.app), which picks the file for your system:

| System | File | What it is |
|---|---|---|
| Linux (x64, glibc 2.35+ and OpenSSL 3) | `sage-<version>-x86_64-linux.tar.gz` | the CLI |
| macOS (Apple silicon) | `sage-<version>-aarch64-macos.tar.gz` | the CLI |
| Windows (x64) | `sage-<version>-x86_64-windows.zip` | the CLI |
| Windows (x64) | `Sage_<version>_x64-setup.exe` | the desktop app |

Unpack the CLI and put `sage` on your `PATH`. Nothing is code-signed: on macOS, run
`xattr -d com.apple.quarantine sage` once; Windows may warn about an unknown publisher. The
standalone `Sage_<version>_x64.exe` runs the desktop app without installing it and needs the
Microsoft Edge WebView2 Runtime.

**From source**, on any platform: install Rust (`rust-toolchain.toml` pins the version; on Debian or
Ubuntu, `scripts/setup-dev.sh` also installs what the desktop app needs), then

```sh
cargo build --release                                  # the CLI: target/release/sage
cargo run -p sage-desktop --target-dir target/desktop  # the desktop app
```

## Start

In a project folder, run `sage`, add a key for a provider and say what you want:

```text
$ cd your-project
$ sage
› /connect openrouter        the key is asked for next, hidden, and kept in ~/.sage
› fix the failing tests
```

| Command | |
|---|---|
| `/connect <provider>` | Store an API key. `/connect <url> <model>` adds any OpenAI-compatible endpoint, such as Ollama or LM Studio. |
| `/model`, `/provider`, `/effort` | Switch what runs, and how hard it thinks. `auto`, the default, is the recommended effort: it starts low and rises by itself when the task needs it. |
| `/permissions ask` | Ask before every edit and command. |
| `/goal <condition>` | Keep working until a separate check agrees the condition holds. |
| `/sessions`, `/resume <id>` | Go back to earlier work. Sessions survive restarts; `sage -c` continues the last one in a folder. |
| `/skills <name>` | Switch on a skill: `ui`, `security`, `cleanup`, `debug`, `tests` or `performance`. A project's `.sage/skills` and `AGENTS.md` add their own. |
| `/init`, `/diff`, `/cost`, `/status` | Write an `AGENTS.md`; see what changed, what it cost and what is set. |

`@file` attaches a file, `!command` runs one yourself, Tab completes, Ctrl-C stops a run. `/help`
lists the rest.

> [!WARNING]
> Sage edits files and runs shell commands without asking, unless you set `/permissions ask` (or
> start it with `--ask`). It stops before commands that would wipe a home or root directory, but
> that is a tripwire, not a sandbox: use it in a git repository you can reset, or in a container.
> The commands it runs do not see the provider API keys in your environment (`OPENAI_API_KEY`,
> `DEEPSEEK_API_KEY` and the rest), so that their output cannot carry them to the model.

## Scripts and CI

```sh
sage run -C path/to/project "Fix the failing tests"     # unattended; only a completed task exits 0
sage run --goal "the tests pass and the README names every flag"
sage key deepseek          # store a key (read from stdin), or export DEEPSEEK_API_KEY
sage sessions              # sage show <id> replays one; sage resume <id> "…" continues it
```

A run that hands control back (`yielded`) exits 2. Use `--json` to inspect its status and the separate
`verified` flag when a script requires a checked result.

`--json` prints the result as JSON and `--events` streams every event. `sage --help` lists every
command and option.

## Providers

OpenAI, Anthropic, Google, DeepSeek, OpenRouter, Moonshot, xAI, Groq, Together and Logfare are
built in, and any OpenAI-compatible endpoint can be added. Only DeepSeek V4.1 Flash, GLM-5.3 and GPT-6
Luna have been tested end to end; the others follow their vendors' documentation.

## Desktop app

The same engine in a window: sessions in a sidebar, every step and its output open to inspection,
the diff and Git, skills and settings. Windows builds are released; it builds on Linux and macOS
from source.

## Status

Sage is shared as it is. It works for me and may for you, but there is no support: issues and pull
requests are open to collaborators only, and updates may or may not come. If you want something
changed, fork it; that is what the license is for.

## License

[MIT](LICENSE): use it, change it, fork it, sell it.

## Inside

| Path | |
|---|---|
| `crates/sage-core` | The engine, with no UI dependencies. |
| `crates/sage-cli` | The `sage` binary; `bench/` holds the benchmark tasks. |
| `desktop/` | The Tauri shell and the front end (static, no bundler). |
| `docs/DESIGN.md` | How each mechanism works; modes, goals, sessions, providers, releases. |
| `docs/BENCHMARKS.md` | The benchmark results and how to run them; per-run data in `docs/results/`. |
| `docs/DESKTOP_UI_BRIEF.md` | The desktop app's specification. |

`cargo test --workspace` runs the tests.
