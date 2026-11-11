# Handoff: Presage

Read this first. State: 11 November 2026, end of the sixth session, which stopped when the Logfare
key reached its daily limit. Design notes are in `README.md`; every number is in
`docs/BENCHMARKS.md`; research is in `docs/RESEARCH.md`.

## What the owner wants

1. The harness with the best results on **DeepSeek V4.1 Flash and GLM-5.3**. Results come first;
   cost and time second. Where the two conflict, drop cheapness.
2. Measured against other harnesses, on benchmarks, with the answers graded.
3. About six hand-written skills, selectable when a chat is opened or with a button next to the
   model selector, revised against tests.
4. A strong system prompt / AGENTS.md, configurable in the harness.
5. Memory only if it helps; otherwise remove it.
6. A UI without explanatory tips: only what is needed.

He is terse and watches usage. Work in few turns, report numbers whichever way they fall. For
important work he wants Claude Fable 5.2 subagents; for tedious reading Claude Haiku 5.5, checked
by Fable 5.2.

## Done this session (branch `hoplite/poseidonia-1c69adaf`, PR #5, tests pass)

- **Desktop UI.** The UI agent of the fifth session had died before committing and nothing of it
  was on GitHub. Its edits were recovered from the transcript, replayed and finished: skills button
  next to the model selector (new session and follow-up), settings for skills, system prompt and
  instructions, explanatory text removed. `docs/DESKTOP_UI_BRIEF.md` describes the result and how
  the screenshots are taken.
- **Finish gate bug**: a check behind `cd dir &&` did not count, and `finish` was refused until the
  turn limit. Fixed; a third refusal for the same change is accepted as unverified.
- **GLM-5.3 effort on Logfare**: only a top-level `reasoning_effort` of `low`, `high` or `max` is
  followed there; the harness sent `reasoning: {effort}` and GLM-5.3 thought at `max` in every run
  before this session. New dialect `Logfare` sends both. A reply that only reasons is cut at about
  14,000 tokens and asked for again one effort level lower.
- **Survey** (`survey`, off with `--without survey`): a workspace of up to 40 files is listed with
  line counts and attached up front as far as 1,000 lines go. The budget is in the first message;
  a streak of single-call replies is pointed out; a cut-off reply or a plan in prose no longer ends
  an unattended run.
- **Benchmark**: hard set registered (seven tasks); each run's workspace is a git repository;
  mini-swe-agent and opencode adapters work and no longer carry the task on their command line.
- **Measured** (one run per task and variant; details and caveats in `docs/BENCHMARKS.md`):
  DeepSeek, 17 tasks: Presage 16/17 in 14.3 turns, classic 15/17 in 20.8, mini-swe-agent 16/17 in
  13.9; Presage is the fastest. Presage before this session's changes: 22.7 turns on the skill
  tasks, 20.1 on the hard ones. GLM-5.3, six tasks: Presage 5/6 in 21.0 turns, classic 5/6 in 25.7,
  mini-swe-agent 5/6 in 27.3. Graded answers (of 8): DeepSeek 7.53 / 7.06 / 7.24, GLM-5.3 7.33 /
  5.83 / 6.17 for Presage / classic / mini-swe-agent.

## Not done

1. **Repeats.** Every cell is one run. Nothing above separates the variants on solve rate. The
   first thing to do with a fresh daily quota is `-n 3` on both models.
2. **GLM-5.3 on 11 of the 17 tasks**, and the after-run on its ten skill tasks.
3. **Ablations are half-run** (10 of 17 tasks, DeepSeek only). The review pass cost about two turns
   and rescued nothing; the survey showed no difference in turns; `+skill` solved `restyle` and
   cost turns elsewhere. None of the three is decided. The skills have not been revised against
   their tasks: only `ui` has a task that needs it.
4. **Effort for GLM-5.3**: `max` solved 6/6 against 5/6 for `high` at 40% more time. One run each;
   since results come before time, `max` may be the better default for GLM-5.3. Not decided.
5. **Runs are not isolated from each other.** They share `/tmp` and can see each other's
   workspaces and this repository. Five of 17 classic runs on DeepSeek wandered off after
   `find /`; one read a hidden verifier. `docs/results/2026-11-11-session6/wander.py` finds such
   runs afterwards. Until the bench runs each task in a container or a mount namespace: keep
   `PRESAGE_HOME` and the bench's working directory outside the repository, delete
   `crates/presage-cli/bench` for the duration of a run (`git checkout -- crates/presage-cli/bench`
   brings it back; the binary embeds the fixtures), and remove old `work/` folders first.
6. **Classic gets lost** because it is never told where its workspace is. That is a fault of the
   baseline, not a strength of Presage: give classic the `cwd` line before comparing again.
7. **Memory**: still undecided, leaning towards removal. No live run; the replay result of the
   fifth session stands (`docs/results/memory/lessons-replay.txt`).
8. **opencode, aider and the other adapters**: opencode smoke-tested on two easy tasks. It still
   takes the task on its command line, so an agent's `pkill -f <word of the task>` in any
   concurrent run can kill it (seen on `audit` and `audit-clean`); mini-swe-agent now takes the
   task through the environment. The rest are untested.
9. `restyle` cannot be solved from its prompt alone: the spacing and type-scale limits the hidden
   check enforces are stated only in the `ui` skill. Either say so in the task or keep it as the
   skill's test and leave it out of skill-less comparisons.

## Suggested order

1. `-n 3`, both models, `--tasks skill,hard --variants presage,classic,ext:mini` with the
   precautions of point 5. About 5,000 requests: half a daily quota.
2. `no-review`, `no-survey`, `presage+skill`, `presage+effort:max` on the same tasks; then decide
   review, survey and the GLM-5.3 default.
3. Isolation per run in the bench; `cwd` line for classic.
4. Revise each skill until `+skill` wins on its tasks.
5. Finish or drop the memory experiment; remove `lessons.rs` if it shows nothing.

## Practical notes

- `export PATH="$HOME/.cargo/bin:$PATH"`; `cargo test`; desktop with `--target-dir target/desktop`.
- **Logfare limits for this key: 12 requests in flight, 2,000 an hour (resets on the hour), 10,000
  a day (resets at 00:00 UTC).** A run is about 20 requests. Five workers stay under the hourly
  limit; more only produces 429s that eat the time budget.
- The sandbox can be replaced without warning and then loses everything outside the checkout.
  Keep benchmark state under `target/scratch/` (ignored by git), not in `/tmp`.
- Tool output over about 2,000 tokens is summarised lossily: read in small ranges.
- The command policy blocks heredocs into interpreters and running scripts written during the
  session with `sh`; write files with the file tool and run `python3 file.py` or plain commands.
  `pkill -f <pattern>` kills the shell that runs it if the pattern occurs in its own command line.
- Subagents may edit files outside what they were asked to (one changed `scripts/setup-dev.sh`
  while grading): check `git status` after each.
- The Logfare key was pasted in chat again. It is not in the repository. Rotate it and set
  `LOGFARE_API_KEY` as a project variable.
