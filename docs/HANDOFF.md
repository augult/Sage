# Handoff: Presage

Read this first. State: 11 November 2026, end of the fifth session, which the owner asked to wrap
up early because usage ran out. Earlier design notes are in `README.md`; numbers from earlier
sessions are in `docs/BENCHMARKS.md`; research is in `docs/RESEARCH.md`.

## What the owner wants

1. The harness with the best results on **DeepSeek V4.1 Flash and GLM-5.3**. Results come first;
   cost and time second. Where the two conflict, drop cheapness.
2. Measured against other harnesses, on benchmarks.
3. About six hand-written skills, selectable when a chat is opened or with a button next to the
   model selector, revised against tests.
4. A strong system prompt / AGENTS.md, configurable in the harness.
5. Memory only if it helps; otherwise remove it.
6. A UI without explanatory tips: only what is needed.

He is terse and watches usage. Work in few turns, report numbers whichever way they fall.

## Done this session (on branch `hoplite/lipara-c05ea87b`, tests pass)

- **Skills** (`crates/presage-core/src/skills.rs`, `crates/presage-core/skills/*.md`): six
  built-ins written by hand: `ui`, `security`, `cleanup`, `debug`, `tests`, `performance`. A skill
  is a markdown file with `name` / `title` / `description` front matter. Project skills
  (`<cwd>/.presage/skills`) override user skills (`~/.presage/skills`), which override built-ins.
  CLI: `presage run --skill security`, `presage skills`, `presage skills show <name>`. At session
  start skills go into the system prompt; picked later they ride with the message.
- **Configurable system prompt**: the template is `crates/presage-core/prompts/system.md`. A user
  copy at `~/.presage/system.md` replaces it; `~/.presage/AGENTS.md` (global) and the project's
  `AGENTS.md` / `CLAUDE.md` are appended. `presage prompt` prints exactly what the model gets;
  `presage run --system-file` overrides per run. The facade has `instructions`,
  `set_system_prompt`, `set_global_instructions`, `skills`, `save_skill`, `delete_skill`,
  `send_with_skills` for a UI.
- **System prompt v2** (the template above; the previous text is `docs/prompts/system-v1.md`):
  adds a "how to work" procedure (understand, reproduce, change, check against the specification
  rather than only the visible tests, review, finish) and rules against fake solutions.
- **Review pass** (feature `review`, `script.rs`): the first `finish` after a change is answered
  once with the task quoted back and a three-point review, instead of being accepted. Costs one
  turn; switch off with `--without review`.
- **Benchmark runner**: task sets (`--tasks easy|hard|skill|all`), variant modifiers (`+skill`,
  `+sys:<file>`, `+effort:<level>`, `+no-<mechanism>`), and **other harnesses as variants**
  (`ext:mini`, `ext:opencode`, `ext:aider`, …) run through `scripts/external/<name>/run.sh` and
  metered by a local proxy so tokens and cost are counted the same way for all
  (`bench/external.rs`, `bench/proxy.rs`; install with `scripts/external/install.sh`).

## Not done: nothing below has a measured result yet

1. **No benchmark was run on the new prompt, the review pass or the skills.** Whether they improve
   results is unproven. `/tmp/presage-bin/v1` (old prompt) and `v2` existed for an A/B but `/tmp`
   does not survive; rebuild v1 from commit `d7e3cfc`.
2. **The skill task set is registered and unmeasured; the hard set is fixtures only.**
   `presage bench --check --tasks skill` passes for ten tasks (`audit`, `audit-clean`, `deadcode`,
   `deadcode-js`, `mutants`, `mutants-js`, `slowpath`, `nplusone`, `heisenbug`, `restyle`), each
   naming its skill, but no live run has been made on them: difficulty is uncalibrated. The hard
   fixtures (`shipquote`, `attempts`, `connpool`, `confdialect`, `rollup`, `jobqueue`, `ttlcache`
   under `crates/presage-cli/bench/`) have `workspace/`, `hidden/`, `solution/` but no entry in
   `src/bench/tasks/hard.rs` (no prompt, check or verify command): the agent writing them died.
   The last two were copied from its working tree unreviewed. Next step: register them, run
   `presage bench --check --tasks hard`, then calibrate both sets.
3. **External harnesses**: adapters exist; the smoke test was still running when the session
   ended. Not verified by the parent. No head-to-head numbers.
4. **Memory**: undecided. `scripts/memory-experiment/` holds an unfinished warm-against-cold
   experiment for `lessons.rs`; no results. Research (`docs/RESEARCH.md`) expects little from it.
5. **Desktop UI**: untouched. No skills button, no settings for skills or the system prompt, and
   the explanatory text is still there (`grep -n "hint\|'note'" desktop/ui/js/views/*.js`). The
   engine API for all of it is in place.
6. `README.md` and `docs/BENCHMARKS.md` do not yet describe skills, the review pass or `ext:`.

## Suggested order

1. Register the hard tasks; `bench --check --tasks hard`.
2. One run: `presage bench -p logfare -m deepseek-v4.1-flash-1017,glm-5.3 --tasks hard
   --variants presage,no-review,presage+sys:docs/prompts/system-v1.md,classic,ext:mini,ext:opencode -n 3`.
   Keep the review pass and prompt v2 only if they raise the solve rate.
3. `--tasks skill --variants presage,presage+skill`: revise each skill until `+skill` wins.
4. Desktop: skills button next to the model selector, settings sections, remove the tips.
5. Finish or drop the memory experiment; remove `lessons.rs` if it shows nothing.

## Practical notes

- `export PATH="$HOME/.cargo/bin:$PATH"`; `cargo test`; desktop with `--target-dir target/desktop`.
- Tool output over about 2,000 tokens is summarised lossily: read in small ranges.
- The command policy blocks heredocs into interpreters and generated commands; write files with
  the file tool and run plain commands.
- Subagents cannot be messaged or stopped by the parent and are capped at four at a time.
- The Logfare key was pasted in chat again. It is not in the repository. Rotate it and set
  `LOGFARE_API_KEY` as a project variable.
