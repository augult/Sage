# Handoff: Sage

Read this first. State: 12 November 2026, after the tenth session, which removed the reflex memory
and the review pass, added `/goal`, made a stored session continuable after a restart, and gave the
harness three modes: Ecosage, Default (the conventional loop, `classic` until now) and Prosage.
**Prosage is built and tested against a scripted model only. No benchmark of it exists**: Logfare
answered every chat request with 502 or 503 from 14:12 UTC to the end of the session.
Design notes are in `README.md`; every number is in `docs/BENCHMARKS.md`; research is in
`docs/RESEARCH.md`.

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

All four are in `Features::OPTIONAL`, so each can be measured alone in either direction.

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
- The Logfare key was pasted in chat again. It is not in the repository. Rotate it and set
  `LOGFARE_API_KEY` as a project variable.
