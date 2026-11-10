# Handoff: Presage

Read this first if you are picking the project up. It is written for someone with no prior context.

## What the user asked for

Lars Landmand (repo owner) asked, over one session, for:

1. An agent harness built from scratch with an outstanding performance/cost ratio, **something new
   that has not been made before**, informed by web research (he had read about the Midas harness).
2. Tuned for the models he mainly uses: **DeepSeek V4.1 Flash and GLM-5.3** (cheap, think for long).
3. **A desktop app**, with a simple but modern UI.
4. **Rust for everything possible** (fast load times and performance).
5. Live testing through **logfare.ai** (free gateway; he supplied an API key in chat).
6. Subagents, if used, routed to `claude-fable-5-2` (he said "fable 5.2 xhigh"; the delegation tool
   only takes a model id, effort cannot be set).

He communicates tersely and steers often. He wants honest status, not optimism.

## Where everything is

- Repo `augult/harnessnew`, branch **`hoplite/axos-0f41d603`**. The repo was empty, so this is its
  only branch and GitHub treats it as the default. **No pull request exists**: there is no base
  branch to open one against. Ask the user whether to create `main` or keep working here.
- Layout: `crates/presage-core` (engine), `crates/presage-cli` (`presage` binary),
  `desktop/src-tauri` + `desktop/ui` (Tauri 2.12 shell, scaffold only), `scripts/setup-dev.sh`,
  `README.md` (design summary, status, live numbers), `docs/DESKTOP_UI_BRIEF.md` (UI spec).
- Start reading at `crates/presage-core/src/lib.rs`, then `script.rs`, `agent.rs`, `harness.rs`.

## The design in one paragraph

On the target models a cached input token costs 1/50 of a fresh one and 1/200 of an output token
(DeepSeek Flash: $0.003 / $0.15 / $0.60 per M; GLM-5.3: $0.26 / $1.40 / $4.40). So the scarce
resource is deliberation (turns × thinking), and the typical failure is an unnoticed error that
compounds. Presage's answer is *expectations*: all tool calls of one reply run as a script that
halts at the first surprise; `shell` calls carry predictions (`expect_exit`, `expect_has`,
`expect_lacks`) and a predicted success is answered with a receipt instead of the output; watches
are standing checks re-run by the harness and reported only on change; `finish` requires a passing
check since the last workspace change. Around that: hash-anchored edits, in-process ripgrep search,
mechanically learned error→fix lessons, an append-only byte-stable context with a cache audit, a
price-aware trim rule, an effort ratchet, tool-call repair. `--classic` turns all of it off for A/B.

## What is done and verified

- Engine + CLI: `cargo test` → **132 passed** (unit tests per module, end-to-end agent runs against a
  scripted provider, the harness facade, the demo scenario).
- `presage demo` plays the scripted three-turn walkthrough through the real harness.
- **Live on GLM-5.3 via Logfare**: runs end to end, task solved, completion verified by the harness.
- Tauri scaffold builds (about 2 min cold) and renders under Xvfb; a screenshot was taken.

## What is not done, in priority order

1. **Desktop UI: not started.** `desktop/ui/index.html` is a placeholder and `src-tauri/src/main.rs`
   has no commands. A subagent was given `docs/DESKTOP_UI_BRIEF.md` but had written nothing when the
   session ended. This is the largest missing piece of what the user asked for.
2. **The performance claim is unproven; current evidence is parity at best.** Five live runs of one
   small task (table in README): Presage 5–6 turns, $0.018–0.022; classic 5–7 turns, $0.013–0.025.
   Causes, both observed directly:
   - GLM-5.3 does **not chain** edit → check → finish in one reply. It batches reads and makes
     predictions, but works step by step. Ideas, untested: have the harness do the chaining itself by
     auto-promoting the first test-like command to a watch (so an edit-only reply comes back with
     the check result attached); a worked example in the first user message; stronger wording.
   - The fixed prefix is ~1,900 tokens against ~900 for classic. Trim tool descriptions, or defer
     `watch`/`job` schemas until first needed (mind that changing the tool list busts the cache).
3. **DeepSeek V4.1 Flash has never answered.** Logfare's route returned 429/502 "overloaded" all
   session. Its dialect (`reasoning_content` must be replayed in tool loops or the API returns 400;
   `thinking` + `reasoning_effort` low/high/max) is implemented from the docs only.
4. **No real benchmark.** Build `presage bench` in Rust (several tasks × modes × repeats, independent
   verification, medians). Do not write the runner as a shell script: see sandbox notes.
5. Durable sandbox setup is not registered with the platform (`project_control settings_update`).
6. Smaller gaps: sessions cannot be resumed after an app restart; Windows shell handling; no OS
   sandbox for commands (only a tripwire for catastrophic ones); effort ratchet and lesson recall
   are tested with the scripted model only; context trimming is deterministic (no LLM summary).

## Facts established by live probing (do not re-derive)

- Logfare: OpenAI-compatible at `https://logfare.ai/v1`, bearer key, models
  `deepseek-v4.1-flash-1017`, `glm-5.3`, `glm-5.3-flash`. It is OpenRouter-shaped: send
  `reasoning: {effort}` + `include_reasoning: true`; reasoning streams as `delta.reasoning`; usage is
  in a final chunk with `prompt_tokens_details.cached_tokens` and
  `completion_tokens_details.reasoning_tokens` (needs `stream_options.include_usage`). Rate limit
  80 requests/min. **It logs every prompt and completion**: only synthetic tasks, no secrets.
- GLM-5.3 returns several tool calls per reply and accepts reasoning echoed back verbatim. It
  sometimes leaks its native call syntax into nested arguments
  (`"expect":"exit</arg_key><arg_value>nonzero"`), which is why predictions are flat string fields.
- The gateway's cache for GLM is unreliable: identical prefixes often report 0 cached tokens. The
  harness's own prefix is byte-stable (the audit says provider-side `miss`, never `bust`).
- GLM-5.3 defaults to effort `max`; `high` uses roughly a third fewer output tokens (vendor figures).

## Sandbox notes that cost time

- Rust is **not preinstalled**. In this session the user approved, explicitly: Rust 1.99.1 via the
  official `rustup-init` binary (checksum-verified), the apt packages in `scripts/setup-dev.sh`, and
  the crates in `Cargo.lock`. A new sandbox needs them again; ask before installing anything beyond
  that list (`tauri-cli` and `tauri-plugin-dialog` were not approved, hence the in-app folder picker).
- Commands the sandbox policy **blocks**: piping to an interpreter or `python3 - <<EOF`; executing a
  shell script you wrote or downloaded (`bash x.sh`); using a variable as the command (`$BIN run`);
  generating files and executing them in one command. What works: write files with the Write tool,
  run `python3 file.py` / `node file.mjs` / literal binary paths. Never `pkill -f` a pattern that
  appears in your own command line.
- Build the desktop crate with `--target-dir target/desktop` so it does not fight the core build for
  the lock. Headless screenshot recipe is in `docs/DESKTOP_UI_BRIEF.md`.
- The Logfare key is **not** in the repository and must not be. Ask the user to set
  `LOGFARE_API_KEY` as a project variable. The key he pasted in chat should be rotated.

## Suggested first moves

1. `cargo test` to confirm the baseline (132 passing).
2. Build the desktop UI from `docs/DESKTOP_UI_BRIEF.md`; verify with screenshots.
3. Implement auto-watch, re-run the live A/B from the README on GLM-5.3, and report the numbers
   whichever way they fall.
