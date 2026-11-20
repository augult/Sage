# Handoff: Sage

## Sixteenth session: an interactive CLI, the licence and 0.3.2 (20 November 2026)

- **`sage` in a terminal is a conversation** (`crates/sage-cli/src/chat.rs`): bare `sage`, `sage chat`
  or `sage -c` (the last session in the folder) / `--resume <id>`. A reedline editor reads messages
  and `/` commands, typing `/` or `@` opens a completion menu (commands with their arguments,
  providers, models, skills, sessions, files), a footer under the line says where the next message
  goes. Commands: help, new, clear, sessions, resume, rename, connect (hidden key entry, or
  `/connect <base-url> <model>` for an endpoint of one's own), provider, model, effort, mode,
  permissions (`ask` gives the core's approval requests a y/n key), skills, goal, init, cd, diff,
  status, cost, verbose, demo, exit. `@path` attaches files, `!cmd` runs a command, a number answers
  the last question's choices, Ctrl-C stops a run, twice at an empty prompt quits.
- Enter on an open menu runs the picked command (`/sta` runs `/status`) but only inserts a picked file
  (`enter_for`); `/connect ` with no argument runs as typed rather than picking the first provider.
- Tested by hand through a real terminal emulator (xterm.js on a pty) with the demo and with a
  stand-in OpenAI-compatible endpoint: approvals, a second message to a session, Ctrl-C mid-command,
  continuing after a restart, answering a question by number. Screenshots:
  `docs/screenshots/cli-commands.png`, `cli-run.png`.
- **MIT licence**; the README says the project is shared as it is. The README also says plainly that
  Sage edits and runs commands without asking unless `/permissions ask`.
- **Release 0.3.2** adds CLI archives for Linux x64, macOS arm64 and Windows x64 to the Windows
  desktop installers (`release.yml`, job `cli`); the release notes are written in the publish job.
- CI on `main` was red on rustfmt only (the mode rename); formatted.
- **Public.** The repository went public on 20 November 2026. Issues and pull requests are for
  collaborators only, which the README's "Status", `CONTRIBUTING.md` and the release notes say;
  Actions minutes cost nothing on a public repository, macOS ones included. The README was
  rewritten for people arriving from outside: what Sage does, the polyglot results on both models,
  downloads per system, the `/` commands, scripting, providers and the desktop app.
  `docs/screenshots/desktop.jpg` is the New session view, taken from the browser fixture with its
  sample session, folder and model renamed; the older screenshots still show the name Presage.
- **Website** (augult/Harness): the download button asks GitHub for the latest release and links
  the file for the visitor's system (Windows the desktop installer, macOS and Linux the CLI); the
  copy of the 0.3.0 installer the site served is gone.
- **macOS, fixed:** the first macOS run of the tests (the release's CLI job) failed three core
  tests that had never run there. All three were faults users would meet, fixed in the code and
  checked on GitHub's `macos-latest` runner (Apple silicon, `/bin/bash` 3.2):
  - `tools::jobs::tests::cd_minus_stays_in_the_session_directory`: bash 3.2 ignores the `OLDPWD`
    it inherits, so `cd -` failed with `OLDPWD not set`. Every command now starts with
    `OLDPWD=$PWD` (`HOME_OLDPWD` in `tools/jobs.rs`).
  - `tools::shell::tests::a_command_handed_over_as_a_job_does_not_take_the_shell_state_back_with_it`:
    bash 3.2 numbers the lines of a `-c` string from 0, so with the state prelude on the command's
    first line an error on line 3 was reported on line 2. `counts_lines_from_zero` asks bash once
    (`echo $LINENO`); where it says 0, the prelude gets a line of its own. (Assigning `LINENO` is
    no way round it: bash 5.2 ignores that for `-c` line numbers.)
  - `harness::persistence_tests::stopping_a_queued_run_cancels_it_before_the_first_request`: the
    session lock lasted until the last copy of its file was closed, and a command being started
    at that moment (between fork and exec) holds one, so a session just closed could read as
    `busy` and refuse to be continued. `SessionLock` now unlocks when dropped, and `held` after it
    looks; `resume::tests::a_dropped_lock_is_free_at_once_while_a_copy_of_its_file_is_still_open`
    holds a copy open across the drop and fails without the fix on Linux too. The same test also
    failed on Linux in the 0.3.2 release run, which shipped with a narrower fix made on `main`
    meanwhile: `held` asks again for up to 50 ms before it calls a lock held. Both stay: the wait
    still covers a holder that was killed and so never let go.
  The release's CLI job tests macOS again and its notes say the tests passed on all three
  platforms. CI also runs on `macos-latest`, with Clippy and tests for `sage-core` and `sage-cli`
  only: the desktop app is not released for macOS, and macOS minutes are billed at ten times the
  Linux rate on a private repository.

## Fifteenth session: performance and correctness audit (18 November 2026)

An audit of what makes a session slower, costlier or wrong, and fixes for all of it. No live runs;
`cargo test` has 273 tests (9 new), Clippy and rustfmt are clean.

- **Change detection** (`workspace.rs`, `state.rs`): the fingerprint skipped edits past its first
  20,000 files (10 of 30 directories of a 30,000-file tree) and, cut by its 400 ms clock, gave
  different answers with nothing changed. It now skips what git ignores, walks every file up to
  100,000, and past that (or past 2 s) takes git's status of the tree instead; outside git it says
  the view is partial (`verified by … (the workspace is too large …)`). It runs on a blocking
  thread. 30,000 files: 72 ms; 111,000 in a repository: 290 ms.
- **Tests that write build output** were counted as a change: never adopted as the watch, and run a
  second time when the model ran them. Ignored files are no change now; a few more derived
  directories (`.build`, `_build`, `.dart_tool`, `.hypothesis`, …) are skipped outright.
- **Time budget** (`agent.rs`, `tools/`, `watches.rs`): it was checked only between replies, so runs
  overran a 20-minute budget by up to 185 s while external harnesses are killed on time. The model
  request, the goal check, commands, watches and `job` waits are now held to what is left.
- **`grep`, `rg`, `diff`, `cmp`, `test` exiting 1** with no prediction are an answer (`[ok] exit 1
  (no match)`), not a surprise that halts the rest of the reply.
- **`job` is always offered**: adding it mid-session re-bought the whole prompt (4 busts in 53
  DeepSeek runs); carrying its ~70 tokens is about 100 times cheaper.
- **Decay** counts the replies the run has left (turn limit and time), never trims on the last
  reply, and counts tool-call arguments in what a trim re-buys.
- **Cost of cut replies**: a reply cut for looping, for the time budget or by the user, and attempts
  that streamed and were then retried, are booked by estimate (prompt and streamed text); one cut
  for overthinking now books its prompt too.
- **Overthinking**: after the third cut in a row the next reply is the run's last (`finish` taken as
  it stands) instead of the fourth cut failing the run unreported.
- **Smaller**: the bench proxy reads Anthropic's usage (cache counted apart from `input_tokens`,
  reported in two parts of a stream); the start-up `git status` runs while the first reply is
  coming; `search` and snapshot writes run off the async runtime; a stream that carries reasoning is
  given up on after 120 s of silence instead of 300 s.

Read this first. State: 17 November 2026, after the fourteenth session, which **took Prosage
out** (next section). What follows about Prosage is the record of earlier sessions.

Before that: state on 14 November 2026, after the thirteenth session. The tenth removed the reflex
memory and the review pass, added `/goal`, made a stored session continuable after a restart, and
gave the harness three modes: Ecosage, Default (the conventional loop, `classic` until now) and
Prosage. The eleventh renamed the product (it was Presage, and Ecosage was `presage`), reworked
Prosage, and measured it live (**Prosage 47/48 against OpenCode's 45/48 and Ecosage's 44/48**).
The twelfth session fixed six structural gaps read off those Prosage runs and tested them offline
across all 24 benchmark tasks. The thirteenth measured that code live (**48/48**) and cut what
Prosage spends beside the run: **23% less cost, 41% less time, still 48/48** (next section).
Design notes are in `README.md`; every number is in `docs/BENCHMARKS.md`; research is in
`docs/RESEARCH.md`.

## Fourteenth session: Prosage taken out, and an audit (17 November 2026)

The Python, Rust and Go benchmark (`docs/BENCHMARKS.md`, first section) had Prosage solve fewer
tasks than Ecosage (41/72 against 44/72) at 2.6 times the cost, and Ecosage tie OpenCode at about
60% of its cost and time. The owner asked for Prosage to be removed.

- **Removed**: the `prosage` mode, and the six mechanisms only it used (`examiner`, `panel`,
  `referee`, `long_thought`, `unhurried`, `routing`), with `examiner.rs`, the readers' file supply
  in `attach.rs`, the settling rule and the examiner gate in `agent.rs` and `script.rs`, the scripted
  provider's answers beside the run, the `side_requests` stat, the bench variant, the CLI help and
  the desktop app's Prosage option. `--with` / `+<mechanism>` now switch any mechanism on, e.g. in
  the conventional loop.
- **Kept working**: a session stored as Prosage resumes as Ecosage, and retired mechanism names in
  its stored setup are dropped (`Features::RETIRED`; tested).
- **Fixed**: a reply cut off for overthinking was priced without the peak-hour multiplier and did
  not mark the session as priced; it is now booked like every other request (`count_usage`).
- **Dead code removed**: `Evidence::resync` and `checked_epoch`, which only the examiner read.
- **Second audit**, the same day: the usage parser was one copy in the provider and a drifted one
  in the bench proxy; Sage's own copy missed `output_tokens_details.reasoning_tokens`, so it now
  reads every shape and the proxy uses it. `kill_group` is shared too, and the CLI no longer
  depends on `libc`. Dead CSS (`.opens-up`), seven needless JS exports and the unreferenced
  `docs/prompts/system-v1.md` are gone, as is `vercel.json` (Sage is not deployed). CI
  (`.github/workflows/ci.yml`, formerly the UI-only check) now runs formatting, Clippy with
  `-D warnings`, the tests and the front end's lint on every pull request and push to `main`, and
  local builds, CI and releases all take the Rust version from `rust-toolchain.toml` (1.100.0).
  The tests of `agent.rs` and `harness.rs` moved to `agent/tests.rs` and `harness/tests.rs`.

## Thirteenth session: Prosage for less (14 November 2026)

**The owner's brief:** the same results or better from Prosage, for less money. Measured live
with the Logfare key (`docs/BENCHMARKS.md`, first section; raw results in
`docs/results/2026-11-14-prosage-cost/`).

| 24 tasks, DeepSeek V4.1 Flash | Solved | Requests | Beside | Time | Cost |
|---|---|---:|---:|---:|---:|
| Prosage as the twelfth session left it, two runs each | 48/48 | 8.9 | 3.4 | 223 s | $2.25 |
| Prosage now, two runs each | 48/48 | 8.3 | 3.7 | 131 s | $1.74 |
| Prosage now with one examiner (`prosage+no-panel`), one run each | 24/24 | 9.2 | 2.5 | 140 s | $0.63 |

**What the first live runs of the twelfth session's code showed.** It holds: 48/48, the worker
at 9 replies on average, no run at the limit, no long tail. What was left to cut was beside the
run: the examiners and referees were 71% of the cost, and a third of that was thought thrown
away (an examiner that reasons to its allowance was cut and asked again from nothing).

**Changed** (`agent.rs`, `examiner.rs`, `provider/openai.rs`):

- `side_request` hands a reply that was cut off as all thought its own thought back and tells
  it to answer from it (`examiner::CUT_SHORT`). No examiner was lost to thinking afterwards;
  before, 16 of 96 were, and 37 more thought everything twice.
- Examiners think at the session's effort (`examiner_effort`) and for about 16,000 tokens at
  most before the script begins (`EXAM_THOUGHT_CHARS`). Referees are unchanged: `high`, 24,000.
- `Agent::rule` asks the referees one after the other (`Agent::reading`), a case going on only
  while it can still be confirmed. The rule itself is the same.
- The thought of a request beside the run that fails is counted (`count_lost_thought`). The
  old figures left it out, so the baseline above is understated by about $0.46.
- A stream that ends with no finish reason and no `[DONE]` is a failed attempt and is asked
  for again (`Accum::broke_off`). The gateway did this for an hour on the 14th; such a reply
  had counted as an examiner's "no script" or as an empty reply of the worker.

**Left as it is, on purpose:**

- The number of cases an examiner writes. A budget of 30 lost `confdialect` twice in two
  runs: the worker is told when an examination holds nothing and finishes on it, so a thin
  script costs tasks. Whoever shortens the scripts has to change what `EXAM_STANDS` says too.
- Both examiners. One alone solved 24/24 for 28% less, on one run per task; the second found
  three of seven candidate gaps. `-n 2 --variants prosage,prosage+no-panel` is the run that
  would decide it.
- The worker's side (long thought, no hurry, the stopping rule): 29% of the cost now, and what
  solves `confdialect` and `restyle`.

**Not done:** Ecosage, Default and OpenCode were not run again, so the twelfth session's
command below is still owed for them. Thinking cannot be switched off for DeepSeek through
Logfare (`reasoning: {enabled: false}` still thinks), so `Effort::Off` is `low` there in
all but name.

**How the runs were made:** one account, the arms of a comparison side by side with `-j 2`
or `-j 3` each (five to six runs at once), `--max-turns 60 --time-budget 1200 --keep`, state
under `target/scratch/`. About 2,800 requests and $8.50 for the whole session, replays and
abandoned arms included. Check the gateway before a long run: two streams of a `high`
request side by side that both end with a finish reason.

**Checked:** `cargo test` (291 tests), `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, `sage bench --check --tasks all` (24/24), `sage bench --scripted --tasks
all --variants ecosage,default,prosage` (72/72). The binary measured is the tree as committed.

## Twelfth session: Prosage tightened from the live traces (13 November 2026)

No live provider key was available (Logfare daily limit reached). Reading the stored Prosage,
Ecosage and OpenCode runs (`docs/results/2026-11-12-prosage/`, `2026-11-12-opencode/`) against the
code exposed why Prosage lost `audit` once, burned 60 turns (`outcome: budget`, `side_requests: 1`,
examiners never run) in 3 of 4 `restyle` runs, jumped to 57–60 turns in Sample 3 on `ttlcache`,
`nplusone` and `rollup`, and confirmed only 1 of 73 referee cases:

- **Mechanical task-contract guard (`contract.rs`, `Features.contract`, on in Ecosage and Prosage):**
  snapshots files and folders named in `Do not edit/change …` clauses (46/46 `restore` files
  across the 24 tasks) and checks explicitly requested deliverable files (`Write \`findings.json\``,
  `Create \`roman.py\``, etc.) before `finish` stands—even when `epoch == 0`, closing the `audit` /
  `audit-clean` failure where a run finishes in prose or writes `notesvc/findings.json` instead of
  `./findings.json`. Costs zero model tokens; zero false positives on all 24 reference solutions.
- **Watchless examination and verification (`agent.rs`, `script.rs`, `state.rs`):** on tasks with
  no watched test runner (`restyle`, `audit`, `roman`), unpredicted read-only check commands that
  exit 0 now trigger `early_exam`, passing examiner suites credit `evidence.passed` (remembering
  that `Watches::all_pass` is false when `watches` is empty), and watchless `finish` passes through
  to `exam_check` (falling back to the evidence gate if no examiner produced passing cases).
- **Progress-aware settling & de-conflicted coverage notes (`agent.rs`):** `settling` now counts
  consecutive surprise-free replies on the *same* `epoch` and resets whenever the worker edits a
  file or hits a failing check of its own, removing the risk of cutting `confdialect` mid-fix;
  `COVERAGE_NOTE` is suppressed on turns where `early_exam` already answered (`EXAM_STANDS` or a
  hold), removing the contradictory instruction that drove 57–60 turn tails in Sample 3.
- **Full-workspace sight for tool-less readers (`attach::for_readers`):** files omitted from the
  worker's `<files>` by the 250-line or 24-file caps (`notesvc/app.py` at 262 lines in `audit`,
  files 25–27 in `deadcode`, longer `SPEC.md`s) are supplied in `<more_files>` to the examiners
  and referees without growing the worker's per-turn prompt.
- **Tie-breaker referee & tolerant parsing (`agent.rs`, `examiner.rs`):** two `GAP` votes confirm
  at once and any `CASE` vote rejects at once; a split of one `GAP` against `OPEN` (with zero
  `CASE`) asks one tie-breaker referee, requiring 2-of-3 `GAP`. `examiner::parse` and `rulings`
  accept markdown-formatted headers, parenthesised verdicts and two-line blocks, and fenced
  scripts missing `{file}` are repaired once (`REPAIR_FORMAT`).
- **Router headroom, tamper-proof scripts & per-session `TMPDIR` (`agent.rs`, `state.rs`,
  `script.rs`):** `ROUTE_MAX_TOKENS` raised from 400 to 2,048 for providers whose thinking cannot
  be switched off; `run_script` restores `exam_*.ext` from memory before running and `edit`/`write`
  refuse paths inside `sess.dir`; each session sets its own `TMPDIR` outside `sess.dir`.
- **Zero-cost accuracy lifts for Ecosage and Default (`prompts/system.md`, `agent.rs`,
  `provider/openai.rs`, `tools/edit.rs`):** Ecosage now carries `contract` at zero token cost,
  qualifies its passing-check chain hint so spec-heavy tasks (`confdialect`) check untested rules
  before finishing, raises `REASONING_CAP` from 56k to 64k chars (~16k tokens) so 13k–15k Turn-1
  thoughts are not cut and retried, and carries one compact HTML/CSS token-and-accessibility rule
  in `system.md` (addressing `restyle` without adding a side-request or full skill prompt; prefix
  stays well under the 2,300-token cap). Unattended Default (`classic`) runs now nudge a prose-only
  plan reply once instead of yielding immediately, keep whole-file `seen` coverage across `edit`s
  so `read -> edit -> write` needs no `overwrite: true` retry, and give plain numbered context
  instead of anchor instructions when `edit` fails with `anchors == false`.

**Checked:** `cargo test` (286 tests: 265 in `sage-core`, 21 in `sage-cli`), `cargo clippy --lib
--bins -- -D warnings`, `sage bench --check --tasks all` (24/24) and `sage bench --scripted
--tasks all --variants ecosage,default,prosage` (72/72), plus multi-mode failure-recovery runs on
`audit`, `audit-clean`, `restyle`, `pager` and `confdialect`. **Not checked:** live Logfare runs
(daily limit reached). First thing to run when the key resets:

```sh
sage bench -p logfare -m deepseek-v4.1-flash-1017 --tasks all -n 2 -j 1 --max-turns 60 --time-budget 1200 \
  --variants default,ecosage,no-contract,prosage,prosage+no-examiner,ext:opencode
```

## Eleventh session: the rename, and Prosage reworked (12 November 2026)

Branch `hoplite/massalia-854d4b32`. The product was renamed from Presage to Sage and its cheap
mode to Ecosage (PR #19, merged; nothing of the old name is accepted any more, see "Practical
notes"). Then Prosage was reworked with Logfare still down (PR #20), on what is in
`docs/RESEARCH.md`, section 3. Then Logfare came back and it was measured, and changed twice more
on what the runs showed.

**First live results** (`docs/BENCHMARKS.md`, first section; `docs/results/2026-11-12-prosage/`):

| DeepSeek V4.1 Flash, 24 tasks | Solved | Requests | Time (mean) | Cost |
|---|---|---:|---:|---:|
| Ecosage, two runs each | 44/48 | 6.5 | 46 s | $0.43 |
| Prosage as in PR #20, two runs each | 47/48 | 15.1 + 2.6 beside | 261 s | $2.45 |
| OpenCode 1.19.5, two runs each | 45/48 | 10.9 | 92 s | $0.61 |
| Prosage after both changes below, one run each | 24/24 | 19.3 + 4.8 beside | 183 s | $1.32 |

- Prosage solved `confdialect` and `restyle` every time (4/4 each); Ecosage lost each twice.
- **The examiners and referees decided almost nothing**: 73 times asked, one case confirmed.
  The referees set wrong cases aside, which is their job, but what solves the two tasks is the
  rest of Prosage (no hurry, longer thought, the `ui` skill switched on). Measure
  `prosage+no-examiner` before building more on the examiners.
- Changed on these runs: a request beside the run that only reasons is retried with thinking off
  (a third of the examiners were lost that way, and one level lower lost them again); the
  examiners' cases are run as soon as the worker's own checks pass (`Agent::early_exam`: on two
  tasks the worker never called `finish` in sixty replies); a script cut at the output limit is
  asked for again; examiners' and referees' replies are kept in the session's `exam/` folder.
- Changed after reading the logs, **unit-tested and not measured** (the owner stopped the check):
  a stopping rule for an unhurried run (`Agent::settling`: told to finish after 8 replies with
  nothing left failing, last reply after 16); the router picks one skill (`examiner::routed`);
  an examination that holds nothing is said once and without the count of cases set aside
  (`EXAM_STANDS`); the key file is refused to the tools (`script.rs`, `names_key_file`); with
  referees, a script most of whose cases fail goes to them instead of being set aside, which is
  what lost `audit`.
- **First thing to run:** `prosage` on all 24 tasks, `-n 2`, against the 47/48 and 24/24 above.
  The stopping rule is the change that can cost tasks: look at `confdialect`, `rollup`,
  `nplusone` and `ttlcache`, and at runs that end `budget` with "was not declared finished".
- **Open:** Prosage's worker writes its own check scripts into `/tmp`, which runs share.
- **The Logfare key reached the provider in one request** (the run that printed the key file).
  Rotate it.

**How the runs were made:** `sage bench -p logfare -m deepseek-v4.1-flash-1017 --variants
ecosage,prosage,ext:opencode -n 1 -j 1 --max-turns 60 --time-budget 1200`, the 24 tasks split
over two accounts and each half run twice in two more; the Prosage-only samples with `-j 2`. The
whole evening was about 3,000 requests and $6. A Prosage run takes 3 to 15 minutes, so a full
head-to-head is about 100 minutes with four accounts.

What follows was written before those runs.

**The owner's brief:** Prosage is to outperform the other harnesses (OpenCode, Claude Code)
whatever it costs. Ecosage is to come close to them, cheaper and faster. The two differ in what
the harness does and not in the effort: both have to run at any effort, `max` included.

**What was wrong with Prosage as built**, read off the code and the research:

- The worker was the only judge of the examiner's cases. The failure Prosage exists for is a
  worker sure of work that is wrong, and that worker was the one asked whether a failing case
  was right. Published measurements give the other half: cases from a test writer no stronger
  than the worker, handed over unfiltered, lowered a repair agent's result (61.2% to 57.3%).
- The examiner's script was written blind in one reply and first run at `finish`. One that did
  not run was dropped, and the run was Ecosage without the hurry.
- An examiner that thought past the reasoning cap was lost with an error, on just the tasks
  whose text takes the most reading: the worker's own first replies on `confdialect` reach that
  cap at `low`.
- One examiner, one brief.
- A hold showed the script's whole output, and a `finish` after mending one of two cases was
  held again because the output as a whole had changed.

**Changed** (`examiner.rs`, `agent.rs`, `state.rs`; `README.md`, "What Prosage changes"):

- `referee`: two readers rule on each failing case from the text, the script and the case's own
  report, without the work. Only a case both rule GAP holds a `finish` (`Agent::rule`,
  `examiner::rulings`). No ruling, no hold.
- `panel`: a second examiner, briefed for errors, limits and rules that meet (`Angle::Edges`).
- A script that does not run to its count, or most of whose cases fail, or whose failed cases
  cannot be told apart, goes back to its author with its output (`Agent::repair`).
- Failed cases are read one by one, from blocks that open with `FAIL <name>`
  (`examiner::failed_cases`). The worker is shown each once; a `finish` whose failing cases it
  has all seen stands; a case that passed before its last changes is marked as broken by them.
- A request beside the run that only reasons is asked again one level lower (`side_request`).
- The worker is told with its task that the work will be examined (`EXAM_BRIEFING`), as it is
  told of a goal.
- Nothing is asked for beside the run once less than 45 s of a time budget are left
  (`Agent::aside_wait`). An unhurried run waits up to 15 minutes for a script still being written.

**Effort, in every mode:** a level set by hand is kept (`Agent::restore_fixed_effort`). A reply
that only reasons is retried one level lower, and the session is back at its level once the
model has acted. Before, a session set to `max` went to `high` at the first such reply and
stayed there, then to `low` and to `off`. At `max` the reasoning cap is the long one (about
24,000 tokens) in any mode; outside Prosage it was 14,000, which a hard task reaches in its first
reply. Replies that only reasoned are counted three in a row, no longer three per run. `ratchet`
is unchanged and starts at `low` in every mode.

**Decided without a measurement, and the first things to check:**

- Two referees who must agree. One alone would confirm more real gaps and more wrong cases
  (`REFEREES` in `agent.rs`).
- The second examiner. It doubles the cases a run is exposed to; the referees are what is meant
  to keep that from costing tasks that are solved today. `prosage+no-panel` measures it.
- Examiners and referees think at `high` or above, whatever the session's effort.
- Ecosage's loop is unchanged. Its two losing tasks are lost to what makes it cheap
  (`docs/BENCHMARKS.md`, "Where Sage loses"). `ecosage+unhurried` and
  `ecosage+examiner+referee` are the variants that would say what a little of Prosage costs it.
- A follow-up message is still not examined, and a session brought back after a restart has no
  examiners.

**To measure first**, each bench process alone in a Unix account as the ninth session did:

```sh
sage bench -p logfare --tasks confdialect,restyle -n 5 -j 1 --max-turns 60 --time-budget 1800 \
  --variants ecosage,default,prosage,prosage+no-referee,prosage+no-panel,prosage+no-examiner
sage bench -p logfare --tasks all -n 3 -j 1 --max-turns 60 --time-budget 1800 \
  --variants ecosage,prosage,ext:opencode
sage bench -p logfare --tasks all -n 2 -j 1 --max-turns 60 --time-budget 1800 \
  --variants ecosage+effort:max,prosage+effort:max
```

What to read off:

1. Whether `prosage` solves the two tasks and loses none of the other 22. The notes of kind
   `examiner` in a session's log say which cases held a `finish`, which the referees set aside,
   and how the run ended.
2. What the referees are worth. `prosage+no-referee` is Prosage as the tenth session built it
   plus the second examiner; the question is whether it breaks tasks that `prosage` does not.
3. How often a script needs its author (`is asked to correct its script`), and how often it is
   set aside all the same.
4. That a run at `max` stays at `max` (the `effort` events of its log), in both modes.

A Prosage run makes two requests beside its own at the start and up to four per examination, so
`-j 1` or `-j 2` under the limit of 12 requests in flight. The bench's default budget (30
replies, 600 s) is too little for it: a held `finish` costs replies and every ruling costs time.

**Checked:** `cargo test --workspace` (270: core 247, CLI 19, desktop 4). Every path above ran
against a scripted model: referees that agree and that do not, a script corrected by its author,
a case broken by a fix, an examiner cut for overthinking, a run at `max` in both modes. **Not
checked:** anything against a real model; the app by hand.

## Tenth session: three modes, `/goal`, restart (12 November 2026)

Branch `hoplite/barke-8479543e`, three commits: the removal and `/goal`; the restart (its own
section follows this one); the modes.

**The owner's brief:** not one harness that is both cheap and good, but three modes. Ecosage stays
the cheap one. Default is "the default tool usage like a normal harness", for people who want the
app. Prosage is for the result, "not even trying to be cheap", without "insanely long thinking
times". No CI (it would spend Logfare requests on every build). Memory and the review step out,
`/goal` in.

**Removed:** `lessons.rs`, `scripts/memory-experiment/`, the review pass (`Features.review`, its
prompt text, `ecosage+review`). The change report that lived in `review.rs` is `changes.rs` and is
what the goal check reads. Logs and metas that hold the retired events still open.

**`/goal`:** `goal.rs`, `Agent::before_ending` (`agent.rs`). Where a run would end, by an accepted
`finish` or an answer in prose, one request with a fresh context and no tools is asked whether the
goal holds; `NOT MET` sends the model back with the reasons, three times at most per message.
`before_ending` is where every check on an ending lives: the examiner's is called there first.

**Modes:** `Mode` in `state.rs` (`Ecosage`, `Plain`, `Prosage`; `Plain` is what users call
`default`, and `classic` is still read as it). `--mode` in the CLI, `default` and `prosage` as
bench variants, the Mode picker in the app. A session's snapshot names its mode, so it comes back
as what it was.

**Why Prosage is what it is.** With no provider to ask, the stored runs were read again
(`docs/BENCHMARKS.md`, "Where Sage loses"). Sage loses on two tasks, and loses them by
declaring work finished that its own checks passed and the hidden ones did not. The conventional
loop loses neither, in the few runs it has on them, by doing what Sage is built to avoid: twice
the replies, most of them spent on trying things. And of five first replies on `confdialect` this
session, three were cut at the 14,000-token thinking cap and told to stop thinking (the two that
were not had thought for 12,900 and 13,400 tokens). So Prosage is Ecosage with:

- `long_thought`: the cap is about 24,000 tokens (`ChatRequest.reasoning_cap`).
- `unhurried`: no budget line, no tips about saving replies, no half-time notes; passing checks
  are answered with what they leave open (`COVERAGE_NOTE` in `agent.rs`). The wording that was
  being tried when the ninth session's sandbox was replaced is this one's ancestor.
- `examiner` (`examiner.rs`, `Agent::start_exam`/`examine`/`exam_check`): a second reader with a
  fresh context writes a check script from the task and the files of the first message, beside
  the run; an ending that follows changes is held while cases fail that the worker has not been
  shown. The worker rules on each by the text; ending again with the same cases failing stands. A
  script that does not print its count, or fails more than three cases in five the first time, is
  set aside. A hold raises a ratchet effort one level.
- `routing`: one request without thinking picks skills by their descriptions.

All four are in `Features::OPTIONAL`, so each can be measured alone in either direction. (The
eleventh session added `panel` and `referee` and changed how the examiner's cases reach the
worker: see above. Its list of what to measure first replaces the one below.)

**Decided without a measurement, and the first things to check:**

- Prosage stands on Ecosage's loop. The stored runs would as well support standing it on the
  conventional one (`Mode::features` is the one line to change). Run both.
- The effort starts at `low` in every mode. The ninth session measured `ratchet-high` at twice the
  time for no more tasks solved; Prosage was not part of that.
- The examiner reads only the task a session starts with. A later message is not examined, and a
  session brought back after a restart has no examiner.

**To measure first**, each bench process alone in a Unix account as the ninth session did:

```sh
sage bench -p logfare --tasks confdialect,restyle -n 5 -j 1 --max-turns 60 --time-budget 1200 \
  --variants ecosage,default,prosage,prosage+no-examiner,default+examiner+long_thought+routing
sage bench -p logfare --tasks all -n 2 -j 1 --variants ecosage,prosage     # what Prosage breaks
```

About 1,200 and 1,000 requests. What to read off: whether `prosage` beats `ecosage` on the two
tasks and matches it on the other 22; what the examiner alone is worth (`prosage+no-examiner`); how
often a hold is a real gap and how often the examiner's mistake (the `examiner` notes of a
session's log say which cases were held and how the run ended); how long a run waits for the
examiner (`EXAM_WAIT` is 240 s after it was asked).

**Checked:** `cargo test --workspace` (253: core 230, CLI 19, desktop 4); the three modes through
`sage bench --scripted`; the app under xvfb (mode picker, a Prosage session, the restart flow,
the goal rows). Two baseline `confdialect` runs finished before the gateway went down, both
declared done with hidden cases failing: a bad reference in the second item of a list reported on
the first item's line; `;x #y` not cut at the comment; a backslash on the last line not refused.

**Also fixed:** the New session view wrote `false` above "Choose folder…" when there was no recent
folder.

**The Logfare key was pasted in chat again** (twice now). It is in no file of the repository.
Rotate it and set `LOGFARE_API_KEY` as a project variable.

## Continuing sessions after a restart (12 November 2026)

Part of the tenth session, written next to the work on the modes: `agent.rs`, `harness.rs` and
`state.rs` were edited additively on purpose, so that the two merge.

**What it does:** a session stored by an earlier run of the app or the CLI is continued by sending it a
message (`Harness::send_with` brings it back) or by `sage resume <id> [message]`. `README.md` has
the user's side. The engine's side:

- `crates/sage-core/src/resume.rs`: `Snapshot` (what `snapshot.json` holds), `write_atomic`,
  `read` and `SessionLock` (an OS file lock on `session.lock`, with the pid written in it for the
  message of whoever is turned away; `File::try_lock`, so a killed process leaves nothing behind).
- `Agent::checkpoint`/`restore`/`keep_snapshots` (`agent.rs`) and `Session::checkpoint`/`restore` (`state.rs`).
  The snapshot is written after the user message is in the conversation, after each reply, at the end
  of a turn and when a run ends (`save_point`). `Context::answer_dangling` answers the tool calls a
  killed run left open, `Context::restored` takes the stored conversation and its stubs.
- `Harness::resume`, `resume_scripted` (tests, the app's headless hooks), `resume_stand_ins`, `is_live`,
  `close_all`; `send_with` revives on demand. `list`/`meta` give stored sessions the status `resumable`
  or `closed` with `why_closed` (`older`, `simulated`, `busy`, `gone`). `subscribe` on a stored
  session is an error that says to send a message first (reviving on subscribe would take the lock and
  need the key just to look at an old session); `stop` is a no-op unless another process runs it;
  `approve` says nothing is waiting. `Harness` ending (the last handle dropped) ends its sessions
  and frees their locks, `Live::shut`.
- `EventKind::Resumed { cut_off, answered }` marks the restoration in the log; the UI draws it as a row.

**Decisions:** a session without a snapshot is *not* rebuilt from its log: the log lacks the system
prompt, the composed first message (environment, preflight output, attached files) and the harness's
footers, so the model would not see the conversation it had. `seen` and the anchors' histories start
empty (files are shown again); the evidence starts fresh; the watch results are not kept; the `reply`
counter is not (histories are empty); the step and spill counters are, so numbers in the conversation
are not reused. `dialects` is kept (it records whether a file already failed the syntax check before
the tools touched it, which cannot be found out afterwards). Shell exports and the directory live in
`shell.env` and `shell.cwd` in the session directory, which survive: only background jobs are gone.

**Desktop:** a `resumable` session has a working follow-up box and is read from its log until a
message brings it back (`store.resume`); a session that cannot be continued says why in the
inspector (`Why`). `SAGE_UI_REVIVE=<file>` (see `docs/DESKTOP_UI_BRIEF.md`) lets a scripted
session be continued with scripted replies, which is how the restart flow was photographed. The
goal's UI was seen running for the first time: `/goal` and `/goal clear` answer in the open now (they
were folded into "1 note"), and the Economy block shows the goal checks.

**Not done:** nothing was run against a real provider; a restart with the app and the CLI running one
session at once was exercised only with two harnesses in one process (the lock is an OS file lock, so it
holds across processes, but only the tests' same-process case ran).

## Ninth session: against OpenCode (12 November 2026)

Branch `hoplite/kos-bee4c2ad`. DeepSeek V4.1 Flash through Logfare, all 24 tasks; the tables, the
method and the raw results are in `docs/BENCHMARKS.md` and `docs/results/2026-11-12-opencode/`.

**Where it stands:** in the last head-to-head (three runs per task, each run alone in a Unix
account) Sage solved 67/72 in 5.7 requests and 59 s a run, OpenCode 1.19.5 70/72 in 10.8
requests and 116 s, at twice the money. Before the changes below the two were level (45/48 against
44/48, 10.5 requests against 11.0).

**Changed, each on a measurement:**

- *The review pass is opt-in* (`--with review`, `ecosage+review`, `StartRequest.with`). It cost 30
  to 51% more replies and changed no outcome in 96 runs against 96. This is the question the
  seventh session left to measure, and its own bar ("within one turn of `no-review`") was missed
  by two to four. `Features::OPTIONAL` lists what is off unless asked for.
- *The effort ratchet starts at `low`* (`ratchet` and Auto now mean `ratchet-low`; the old start
  is `ratchet-high`). As many tasks solved, about a quarter fewer replies and half the time on
  DeepSeek, half the replies on six tasks with GLM-5.3. A reply that only reasoned no longer
  switches thinking off the first time (`less_thinking` in `agent.rs`): from `low` that had left
  the hardest task without any thinking for the rest of the run.
- *The watch* stays on the test command the task names (`Watch.named`), and a throwaway script is
  never promoted to it (`throwaway` in `watches.rs`). It had been moved to another command, often
  a scratch one, in a fifth to a quarter of the runs.
- *`write`* replaces a file the model has been shown in full without `overwrite:true`
  (`shown_whole` in `tools/write.rs`).
- *The bench's proxy* waits for a request in flight before closing a run, which fixes the test
  that failed in the sandbox of the eighth session's last change.

**Still worse than OpenCode, and not fixed:**

- `confdialect`: 7/20 over the day against OpenCode's 5/5. Sage's runs declare it done after 5
  to 18 replies with one or two of 39 hidden cases wrong; OpenCode works on it for 25 to 30
  requests. The review pass did not rescue it (1/2 and 0/2 with it). One thing was tried and not
  finished: the note "every check passes, so `finish` is accepted now" comes right before the
  early `finish` in the failing runs, and a wording that asks for the specification to be covered
  first was running, ten runs against ten, when the sandbox was replaced and its results were
  lost. The wording is not in the code. It is the first thing to try again.
- `restyle`: 8/20 against 4/5, failed on the hidden check's spacing and type scale. Six of the
  eight were in one 48-minute window, with every variant, which nothing in the sessions explains.
  The `ui` skill solves it; whether a skill should switch itself on is still an open question.

**Not done:** grading of the answers; GLM-5.3 beyond six tasks; a run of the other adapters
(mini-swe-agent, aider, qwen, goose, codex, pi) against the new defaults; a comparison with
classic after the changes. **How the runs were isolated**, for whoever repeats them: an account
per parallel run (`useradd`), the key stored per account with `sage key logfare`, the
repository `chmod 700`, the binary and `scripts/external` copied to a place all accounts can
read, `BENCH_EXT_HOME` pointing at one installation of the harnesses, and `/tmp/opencode` made
world-writable with the sticky bit, because OpenCode's first run as root leaves it owned by root.
Run long benchmarks detached (`setsid nohup`): a command that outlives its tool call is killed
with it. About 5,600 of the day's 10,000 requests were used.

## Eighth session: the desktop redesign (11 November 2026)

No benchmark was run and nothing in the agent loop changed. Branch `hoplite/massalia-4feee274`.

**Look:** the app no longer reads the desktop wallpaper or the window's place (`glass.js` and the
`wallpaper` and `window_place` commands are gone). It brings its own background: four paintings
made by scripts in `desktop/art` (Ember, Orchid, Tide, Graphite; or None for flat panes), pressed
into a light or a dark band behind translucent panes, with an ordered dither drifting over them
(`backdrop.js`, `dither.js`, `themes.js`). Appearance (System, Light, Dark), background and motion
are in Settings; the background can also be changed from the New session view. The interface
itself has no hue, and corners are 2 to 4px. The one colour besides a failure's is an edit's: green
for lines added, red for lines removed.

**Conversation:** one grey bar per stretch of work is the master toggle of every tool call under
it. A row is one line about what the tool did and opens the same way for every tool: the input
of the call as JSON, then its output, verbatim or, where it has a shape, as that shape (an edit as
a unified or split before and after, a web search as its sources). Replies are set as markdown. A card under the result lists the files the run edited. Stop is in the follow-up box.

**Engine:** `step_end` carries a `change` (path, lines added and removed, hunks) for `edit` and
`write`, made by the new `sage-core/src/diff.rs`.

**Push:** a session whose folder is a git repository has a Push button ("Push to GitHub" on a
github.com remote). `git_state` and `git_push` (`desktop/src-tauri/src/git.rs`) run the user's
own git: commit what is uncommitted, push the branch.

**Vercel:** a Vercel project is connected to the repository and failed on every commit, there
being nothing in it to deploy; `vercel.json` now tells it not to.

**Checked:** `cargo test --workspace` (213); every view under xvfb, in all four paintings and both
appearances (`docs/screenshots/`, hooks in `docs/DESKTOP_UI_BRIEF.md`); a push from the dialog to
a bare repository on the same machine, which received the commit. **Not checked:** a push to
github.com itself; the sources view against a real search (the engine has no web search tool, so
its screenshot is of a step added to a log by hand); the app driven by hand, on Windows or macOS.

**After the owner ran it on Windows** (same day, PR after #8):

- *Providers.* Ten more built in, each through its OpenAI-compatible endpoint: OpenAI, Anthropic,
  Google Gemini, xAI, Mistral, Groq, Together, Fireworks, and longer model lists for OpenRouter.
  `provider/openai.rs` has a dialect for each, written from the vendor's documentation and
  covered by tests of the request it builds: which token limit it takes, how it spells an
  effort, whether reasoning goes back, Gemini's thought signatures on tool calls, Mistral's
  chunked `content`. **None of them has been called.** The first live run with each is the test.
  Anthropic's compatible endpoint returns no thinking and does not cache prompts, which costs
  this harness its economy there: a native client would be the real fix.
- *Windows paths.* `std::fs::canonicalize` answers `\\?\C:\…` there. The engine now strips the
  prefix (`util::canonical`): it showed in the header, and `cmd.exe` cannot start in such a folder.
- *Settings.* No Save buttons and no "Saved": choices and keys are kept at once, text files are
  drafts that a reminder asks about on leaving Settings or closing the window. A provider with a
  key is marked, one without is greyed; provider and model defaults are dropdowns. The effort
  `ratchet` is shown as Auto. Dither can be switched off.
- *Session.* A provider error is shown in the open while the engine retries (it retried a 429
  silently, behind a bar that read "Thinking"). No result line after a stop, no expand-all button.
- *Browse* opens the system's folder dialog (`tauri-plugin-dialog`); the in-app picker is gone.

Not checked: any of this on Windows or macOS; the folder dialog beyond seeing GTK's open under
xvfb; the reminder against a real window manager's close button (the app's own button was used).

**Panes, providers and the chat bar's buttons** (12 November, branch `hoplite/kos-bee4c2ad`):

- *Panes.* The sidebar and the inspector each have a button that puts them away and brings them
  back at once, with no movement (`ui/js/panes.js`, kept by the window). The caption buttons are
  the window's now, not the inspector's: with it away they sit in the sheet's corner.
- *Providers.* Ten built in, in this order: OpenAI, Anthropic, Google AI, DeepSeek, OpenRouter,
  Moonshot Kimi, xAI, Groq, Together AI, Logfare. Mistral, Fireworks and both Z.ai endpoints are
  gone from the list (their dialects remain, for an endpoint in `config.json` that names one); a
  default saved for one of them gives way to the first provider that has a key. Moonshot is new,
  with a dialect written from its documentation and, like the rest, **never called**. Settings
  can add any number of OpenAI-compatible endpoints of the user's own and delete them again
  (`added` in `config.json`, ids `custom-<name>`).
- *Chat bar.* Send is the Return key's arrow on a square with a control's corners; Stop takes its
  place during a run, the same square on a quieter fill.

Checked: `cargo test --workspace` (221) and each of the above under xvfb, light and dark. The
bench test `an_external_harness_is_run_metered_and_verified_like_sage` was unreliable in the
sandbox this was made in: it passed in a full run and failed when run on its own, before these
changes as well (the ninth session found the cause and fixed it). Not checked: the caption buttons' reach to
the window's edge with the inspector away, which needs a pointer; anything on Windows or macOS.
`docs/screenshots/` predates all three.

**The chat bar on the glass** (13 November, branch `hoplite/sparta-lakedaimon-ceb6af95`):

- *New session.* The view opens on the bare glass, with the mark, one line and the chat bar in its
  middle. The folder is a picker under the bar, beside the mode and the autonomy.
- *Chat bar.* A pane that frosts what is behind it (`backdrop-filter`, which the brief had ruled
  out: the owner asked for it, and the brief now names the bar as the one exception). The effort
  and the model are in the bar, with a microphone; Send is grey until there is text to send.
- *Skills.* No button: `/` in the box lists them, `/<name>` switches one on and `/skills` opens the
  dialog. Those in use are chips in the bar.
- *Microphone.* Dictation through the webview's own Web Speech API (`ui/js/views/dictate.js`).
  `src-tauri/Info.plist` says what macOS wants said before it grants the microphone.

Checked: the real app under xvfb (the new view with the panes there and away, the `/` menu in a
new session and in a running one, typed commands, the dialog, Settings), and the front end in
Chrome with a stand-in for the shell (the pickers, the folder menu, Default mode, no key, light
and dark, the smallest window, dictation against a stand-in recogniser and against Chrome's
refusal of the microphone). Not checked: dictation with a voice, on any system (WebKitGTK, which
is what xvfb runs, has no recogniser; WebView2 has one, with an open report of `network` errors
since runtime 153, MicrosoftEdge/WebView2Feedback#5724); anything on Windows or macOS.
`docs/screenshots/` predates this too.

**The window around it** (13 November, same branch):

- *Title bar.* A level of its own over the three panels: the sidebar's button, back and forward
  through the views gone through, the menus, the caption buttons. The sidebar no longer carries
  the mark and the name.
- *Menus* (`ui/js/menubar.js`): File, Edit, View, Window, Help, with keys for the first few
  things (the brief lists them). Help plays the walkthrough, which had no button.
- *Panels.* The sheet runs to the window's right edge and foot; the inspector is away until asked
  for (`sage.inspector` is `on` only once the user has opened it).
- *Backgrounds.* Each panel shows the background or not: Settings has the window in small, cut
  20/60/20, each part a switch. In #26 only the middle one was on at first; all three are now
  (next entry). The dither stands still while no bare glass is in view.

Checked: the real app under xvfb (new session, a session with the inspector away and there,
Settings, an open menu, Ctrl+comma), and the front end in Chrome with a stand-in for the shell
(every menu and its entries, the Edit commands on the task box, the keys, back and forward, the
three panel switches in every combination that matters, no background, the sidebar away). Not
checked: dragging the window by the title bar and double-clicking it, which need a pointer; Paste
from the Edit menu, which a webview may refuse (it then says to use the key); anything on Windows
or macOS.

**Sessions renamed and deleted; what a window starts as** (14 November, same branch):

- *Sessions.* A right-click on a session in the sidebar opens a menu: Open, Rename, Copy folder
  path, Stop while it runs, Delete. Until now only the Code tab's files had one, and a session
  showed the webview's own (Back, Refresh, Print). The engine has `Harness::rename` and
  `Harness::delete` for it (`rename_session`, `delete_session` in the shell). Delete removes the
  session's own directory under the data folder and nothing else; it is refused while the session
  runs and for one another process has. The file tree and the sidebar share one menu
  (`contextMenu` in `ui/js/views/common.js`).
- *Defaults.* A window nobody has set up opens on Orchid, in all three panels, with the dither
  off (it was Ember, the middle panel alone, dither on). What was chosen in Settings holds.
- *Panels in Settings.* Each part of the small window shows its own share of the picture inside
  its own border; the chat bar drawn in it and the tooltips are gone.

Checked: `cargo test --workspace`; in the real app under xvfb, a stored session renamed and one
deleted, with its directory gone from disk and the folder it worked in untouched, the Code tab's
menu still opening, and a first launch with the new defaults; in Chrome with a stand-in for the
shell, the menu by pointer and by keyboard, Stop, the two dialogs and their errors, and back and
forward after a delete. Not checked: deleting a session whose background jobs are still running;
anything on Windows or macOS.

## Seventh session: changes made without a benchmark (11 November 2026)

The Logfare key was at its limit, so nothing below is measured. Each change rests on the recorded
runs of the sixth session, on a defect reproduced in a test, or on both; each is covered by tests
(`cargo test`: 205). Branch `hoplite/epidamnos-dyrrhachion-5e777d04`.

**What the recorded runs said** (`docs/results/2026-11-11-session6/`, DeepSeek, 17 tasks):

- Output is 69% of the cost, 69% of it reasoning; fresh input is 29%, about half of that from
  provider cache misses (28 in 17 runs) and not from anything the harness sent. Cached input is 3%.
- Sage sends 1.4 calls per reply (classic 1.3): its fewer turns come from what is in the first
  message, not from scripts. It writes 82% more output per turn than classic and 25% more in total.
- Every point Sage lost in the graded answers on a solved task was lost the same way: the run
  reached the turn limit and ended on the harness's "Stopped: the turn limit was reached" instead
  of a report of its own (`confdialect`, `mutants`; `connpool` on GLM-5.3).

**Changed for results:**

- **The last reply of the budget.** It is announced one reply ahead ("Your next reply is the last
  one the budget allows…"), by turns or by the time left. In it `finish` is taken as it stands: no
  gate refusal, no review, and not skipped behind a failed step; it is marked verified or
  unverified by what is known. Prose in the last reply is the report rather than the occasion for
  a reminder nobody can act on. `agent.rs` (`last_reply`), `script.rs` (`landing`).
- **Anchors are carried, not searched for.** A reply that edits a file twice sent both anchors
  before either edit ran; the second used to be looked up by its two-letter hash within 60 lines.
  Reproduced: a range ending on `}` stopped at an earlier brace; an anchor for `return None` hit
  its twin in another method; 15% of stale anchors were accepted on an unrelated line. All three
  reported `[ok]`. The harness now keeps the line shifts of its own edits per file and translates
  an anchor through them (`anchors.rs`: `Splice`, `History`, `resolve_range`). A file changed by
  anything else has no known past; there a single anchor is looked for 3 lines either way, a range
  60 lines with both ends at the same distance.
- **Failure output keeps its end.** When the output fitted by line count but not by size, the cut
  came off the end: the assertion line was the part left out (`text.rs`, `bound`).
- **Tool defects reproduced by review and fixed**, each with a test: the syntax check called every
  edit to JSON-with-comments or JSX a surprise (it now reports only what an edit broke); `search`
  reported no matches for pasted code such as `divide(a, b)` and rejected `*.{rs,toml}`; lines a
  long snippet left out were recorded as shown, so reading them answered "[unchanged]"; an
  `expect_exit` the harness could not read ("not 0") was held to exit 0; `grep … | cat` and
  `diff … | head` were reported as failed pipelines; `grep 'startup|shutdown'` was refused as a
  shutdown; `read symbol` cut Python functions at an f-string and long functions silently at 150
  lines; mixed line endings were rewritten and a byte-order mark dropped; a command that outlived
  its timeout erased later `export`s when it ended; bash syntax errors were reported three lines
  late; a call cut off by a stream that ended without a finish reason was repaired and run.

**Changed for cost** (secondary, and only where nothing is given up):

- **The review carries what there is to review.** The first `finish` after a change still comes
  back once, now with `git status` and the diff of what the session changed (anything already
  uncommitted before it is left out, so that the model cannot take the user's work for its own and
  undo it), or the list of edited files outside a repository. The system prompt no longer asks for
  a `git diff` before `finish`: that was a second review, a reply earlier and without the diff. In
  the ablation the review cost about two turns; this should make it one. `review.rs`.
- No advice on batching once the run is at its last reply.

**Fairness of the baseline:** classic is now told its working directory (point 6 below, done).

**Removed:** `--ask` on the CLI (it never asked: the CLI has nobody to ask, and the flag only
switched preflight off); `Harness::send`, `Usage::add`, `locate::is_source_path`, `Job.cmd`,
`Bounded.total_lines`; the unused dependencies `anyhow` and `grep-matcher` and tokio's `fs`; three
compiled Python files committed with the `confdialect` fixture. Clippy is clean.

**Memory:** one defect fixed (a lesson was offered for the very command that had just failed: the
false hints in `docs/results/memory/lessons-replay.txt` at "a real bug that raises"). Still
undecided, still without a live run.

**Not changed, on purpose:** the effort default, the survey, the skills, and whether the review
stays at all. Each needs the ablation; none can be settled by reading.

**Desktop:** the grain over the panes is gone (`glass.js`, `glass.css`); the screenshots were taken
again. Found while taking them and fixed: after a follow-up whose run ended between two redraws,
Send stayed disabled until the session was opened again (3 of 8 scripted runs before, 0 of 8
after). Still nobody has driven the app by hand.

**Desktop, the conversation:** the timeline shows only what was said (the task, reply text, the
closing summary and one line for the outcome). Each turn has one line for what it did ("Read 2
files, ran a command · 1 surprise") that unfolds into the tree of its tool calls, and each call
into its prediction and observation; nothing is open by default, and the main pane of a session is
no longer tinted. `docs/DESKTOP_UI_BRIEF.md` has the rules; `desktop/ui/js/activity.js` writes the
lines. Checked under xvfb only (screenshots of the folded, unfolded, running and
waiting-for-approval states, and a recording of the folds sliding).

**To measure first** with a fresh quota, before anything else is touched: `-n 3` on
`--tasks skill,hard --variants ecosage,no-review,classic,ext:mini`, both models. Three things to
look for: runs that end at the turn limit now end `done` with a summary of their own; turns per
run with the review are within one of `no-review`; `repairs` and edit errors per run do not rise
(the anchor resolver refuses where it used to guess).

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

## Done in the sixth session (branch `hoplite/poseidonia-1c69adaf`, PR #5)

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
  mini-swe-agent and opencode adapters work; mini-swe-agent no longer carries the task on its
  command line (opencode still does, see point 8 below).
- **Measured** (one run per task and variant; details and caveats in `docs/BENCHMARKS.md`):
  DeepSeek, 17 tasks: Sage 16/17 in 14.3 turns, classic 15/17 in 20.8, mini-swe-agent 16/17 in
  13.9; Sage is the fastest. Sage before this session's changes: 22.7 turns on the skill
  tasks, 20.1 on the hard ones. GLM-5.3, six tasks: Sage 5/6 in 21.0 turns, classic 5/6 in 25.7,
  mini-swe-agent 5/6 in 27.3. Graded answers (of 8): DeepSeek 7.53 / 7.06 / 7.24, GLM-5.3 7.33 /
  5.83 / 6.17 for Sage / classic / mini-swe-agent.

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
   `SAGE_HOME` and the bench's working directory outside the repository, delete
   `crates/sage-cli/bench` for the duration of a run (`git checkout -- crates/sage-cli/bench`
   brings it back; the binary embeds the fixtures), and remove old `work/` folders first.
6. **Classic gets lost** (done in the seventh session: it is told its `cwd` now) because it was
   never told where its workspace is. That is a fault of the
   baseline, not a strength of Sage: give classic the `cwd` line before comparing again.
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

1. `-n 3`, both models, `--tasks skill,hard --variants ecosage,classic,ext:mini` with the
   precautions of point 5. About 5,000 requests: half a daily quota.
2. `no-review`, `no-survey`, `ecosage+skill`, `ecosage+effort:max` on the same tasks; then decide
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
- The rename dropped every old name on purpose. An existing install keeps its sessions, keys and
  settings only if `~/.presage` is renamed to `~/.sage` by hand; `PRESAGE_HOME`, `PRESAGE_API_KEY`,
  `.presage` project files and `--mode presage` are no longer read. The stored results under
  `docs/results/` were renamed with the rest: what they call `ecosage` was run as `presage`.
- The Logfare key was pasted in chat again. It is not in the repository. Rotate it and set
  `LOGFARE_API_KEY` as a project variable.
