# Benchmarks

`sage bench` runs 7 small synthetic tasks (Python and shell: fix bugs, implement a function from
a README, write a module that has no tests) under several harness variants. Every run starts from a
fresh copy of the task. When the agent is done, the files it was told to leave alone are put back,
held-out checks are added and the task's verification is run. Only that counts as solved.
`sage bench --check` proves that each task fails as shipped and passes with its reference
solution (`--tasks all` for all 24; without it, the seven of the `easy` set).

All runs below: Logfare gateway, effort `ratchet`, 5 runs per task and variant (35 per variant).
Figures are medians per run unless stated. "No cache" is the list-price cost had the provider
cached nothing, which takes the luck of the cache out of a comparison. Raw results and the CLI's own
tables are in `docs/results/`.

```sh
cargo build
target/debug/sage bench --check --tasks all
LOGFARE_API_KEY=... SAGE_HOME=/tmp/sage-live target/debug/sage bench -p logfare -m glm-5.3 \
  --variants ecosage,no-preflight,no-auto-watch,classic -n 5 -j 3 --out /tmp/glm-5.3.json
```

## Prosage for less, 14 November 2026 (thirteenth session)

What Prosage spends, and what of it buys nothing. DeepSeek V4.1 Flash
(`deepseek-v4.1-flash-1017`), all 24 tasks, effort `ratchet`, 60 replies and 20 minutes per run.
The rows were made the same evening against the same gateway, the arms of one comparison running
side by side, two runs at a time each. Raw results and `pertask.py`, which prints the comparison
by task: `docs/results/2026-11-14-prosage-cost/`.

| 24 tasks | Solved | Requests (mean) | Beside the run | Output tokens | Time (mean) | Cost |
|---|---|---:|---:|---:|---:|---:|
| Prosage before (the twelfth session's code), two runs each | 48/48 | 8.9 | 3.4 | 66,200 | 223 s | $2.25 |
| Examiners at the session's effort, referees one after the other; one run each | 24/24 | 9.5 | 3.5 | 56,800 | 175 s | $1.00 |
| The same, and a cut-off examiner handed its thought back (as it is now); two runs each | 48/48 | 8.3 | 3.7 | 45,500 | 131 s | $1.74 |
| As it is now with one examiner (`prosage+no-panel`); one run each | 24/24 | 9.2 | 2.5 | 29,300 | 140 s | $0.63 |

**Prosage now costs 23% less and takes 41% less time, and solved every run before and after.**
With 48 of 48 on both sides the solve rate cannot show a difference; what is known is that
nothing was lost. Against what was really spent the saving is larger: the old books left out
the thought of an examiner that failed twice (16 of 96 did, about 48,000 tokens each, $0.46 over
the 48 runs). Counting that, it is about a third.

The baseline is the twelfth session's code plus one fix it needed to be measured at all: a
stream the gateway closes in the middle of the thought, with no error, is asked for again
(`provider/openai.rs`). That evening the gateway did this for an hour, and such a reply had
counted as an answer: an examiner's as "no script", the worker's as an empty reply.

### Where the money went

- **The requests beside the run were 71% of the cost**: $0.033 of $0.047 a run, 52,900 output
  tokens against the worker's 13,300. The twelfth session's stopping rule works: the worker
  averaged 9 replies, no run reached the limit, and none of the 57-to-60-reply tails of
  12 November came back.
- **A third of that was thought thrown away.** An examiner thinks before it writes, and on
  DeepSeek the effort setting does not bound it (`reasoning: {enabled: false}` still thinks
  through this gateway). In the baseline's 48 runs, 53 requests to an examiner reasoned for
  96,000 characters with no script begun and were cut. Each was asked again from nothing and
  thought it all a second time: 37 then wrote a script, 16 were cut again and lost, on the tasks
  whose text takes the most reading (of the four examiners of `confdialect`'s two runs, one
  delivered). 23 of 96 examiners delivered nothing.
- **A second referee was asked where the first had settled it.** One `CASE` ruling sets a case
  aside whatever the other reader says, and both were asked at once: in 7 of the baseline's 12
  pairs of replies the first alone had set every case aside.

### What was changed, and what each change is worth

- **A cut-off reply is handed its own thought and told to write from it** (`side_request`).
  Replayed on stored requests: with at least 40,000 characters of its thought in front of it, the
  examiner began its script within 150 characters of new thought in nine cases of eleven, in
  about 20 seconds instead of 100 to 280. With 16,000 to 24,000 characters it thought the whole
  thing again in five of eight, so the cut cannot be made much earlier. In the 48 runs after the
  change no examiner was lost to thinking: 9 of 96 delivered nothing, each by answering `NONE` or
  nothing.
- **An examiner may think for about 16,000 tokens before its script begins, not 24,000**
  (`EXAM_THOUGHT_CHARS`). A script is 5,000 to 9,000 tokens on the hard tasks; behind more
  thought than that it ran into the output limit of 32,000 or never began.
- **An examiner thinks at the session's effort, not at `high`** (`examiner_effort`). At `high`
  17 and 20 replies were cut for thinking in 24 runs each, at `low` 9, and on the seven easy tasks
  the two scripts cost 24,000 tokens against 14,000. Referees stay at `high`.
- **Referees read one after the other** (`Agent::rule`), a case going to the next reader only
  while it can still be confirmed. The rule is unchanged: any `CASE` sets aside, two `GAP`
  confirm, one `GAP` against `OPEN` goes to a tie-breaker. Of 22 examinations of a script after
  the change, 15 took one reply, 6 two and 1 three, where each took two before.
- The thought of a request that fails is now counted in the session's tokens and cost.

By the first two rows, the effort and the referees are worth about a tenth of the cost and the
handed-back thought the rest. The middle row is one run per task.

### What the examiners are worth, measured

Scripts graded against each task's reference solution and against the task as shipped
(`confdialect`, `ttlcache`, `rollup`): a case that fails on the reference is a wrong case, one
that fails only as shipped is what the script can catch.

| Script written | Cases | Wrong | Catch as shipped |
|---|---:|---:|---:|
| In one reply (6 scripts) | 278 | 3 (1.1%) | 22 to 52 each |
| From handed-back thought, 40,000 characters or more (11 scripts) | 762 | 39 (5.1%) | 33 to 60 each |
| Told to write at most 30 cases (10 scripts) | 278 | 5 (1.8%) | 16 to 27 each |

A script written from cut-off thought covers as much and is wrong more often, which is what the
referees are for: they were asked 16 times in the 48 runs after the change and confirmed a case
4 times (`finish` held), against 10 and 3 before. The baseline had no script at all in those
places.

### One examiner or two

The last row is Prosage without its second examiner: every task solved, at 28% less again
($0.026 a run against $0.036). It is not the default. One run per task cannot show that
nothing is lost, and the second examiner does find things of its own: of the seven
examinations after the change in which a referee first ruled `GAP`, three were of its
script. `finish` was held on confirmed cases 4 times in 48 runs with two examiners and 2
times in 24 with one. Two runs per task of `prosage,prosage+no-panel` would say more.

### Tried and not kept

- **A budget of 30 cases** in the examiners' instructions. It cut their output by a third on
  the stored requests and stopped the all-thought replies, and `confdialect` was lost in both
  runs made with it: once all 30 cases passed and the worker, told its work had been examined,
  finished in five replies; once the twelve error cases failed on a bug of the script's own and
  were set aside, which left the errors unexamined. An examination that says "nothing fails" is
  acted on, so a thin one costs tasks. Stopped after 28 runs.
- **Examiners with thinking off.** Not available: the model thinks all the same, and with
  nothing to fall back on 13 of 48 examiners were lost.

### What limits these numbers

- Two runs per task, one model, one evening. Solved means the held-out verification passed.
- The gateway closed streams and answered 502 for about an hour before these runs and now and
  then during them. Both sides had the same fix and ran side by side.
- Times are from a 2-core sandbox with four to six runs at once.
- The grading of scripts is of 27 scripts on three tasks, one request each.

## Prosage, Ecosage and OpenCode, 12 November 2026 (eleventh session)

The first runs of Prosage against a model, made the evening Logfare came back. DeepSeek V4.1
Flash (`deepseek-v4.1-flash-1017`), all 24 tasks, effort `ratchet`, 60 replies and 20 minutes per
run, OpenCode 1.19.5 behind the metering proxy. Four Unix accounts, one run at a time in each, the
three variants alternating; the repository unreadable to all of them. Two runs per task and
variant. Raw results and `table.py`, which prints the table: `docs/results/2026-11-12-prosage/`.

| 24 tasks, two runs each | Solved | Requests (mean) | Beside the run | Time (mean, median) | Cost |
|---|---|---:|---:|---:|---:|
| Ecosage | 44/48 | 6.5 | 0 | 46 s, 25 s | $0.43 |
| Prosage as merged in PR #20 | 47/48 | 15.1 | 2.6 | 261 s, 180 s | $2.45 |
| OpenCode 1.19.5 | 45/48 | 10.9 | 0 | 92 s, 44 s | $0.61 |

Two more samples of Prosage alone, one run per task, made while the head-to-head ran and after
what it showed (below). They ran two at a time in an account of their own, so their times do not
compare with the rows above:

| 24 tasks, one run each | Solved | Requests (mean) | Beside the run | Time (mean, median) | Cost |
|---|---|---:|---:|---:|---:|
| Prosage, side requests retried without thinking | 24/24 | 13.8 | 4.3 | 215 s, 146 s | $1.21 |
| Prosage, examined as soon as its own checks pass | 24/24 | 19.3 | 4.8 | 183 s, 121 s | $1.32 |

- **Prosage lost one run in 96**; Ecosage four in 48 and OpenCode three in 48. Against OpenCode
  that is a lead and not a finding: one run against three in 48 is within chance, and so is the
  95/96 over all three samples.
- **Ecosage solved one run fewer than OpenCode** in half the time, at 60% of the requests and
  70% of the money. Its four losses are the two tasks it always loses: `confdialect` twice and
  `restyle` twice, each time declared done.
- **Prosage solved those two tasks every time** (4/4 each over the three samples). OpenCode lost
  `restyle` once, and `audit` and `audit-clean` once each to a signal that killed it, as in the
  ninth session.
- **Prosage costs four times OpenCode's money and three times its time.** That is its brief.

### What solved the tasks, and what did not

Read from the `examiner` notes of the session logs.

- **The examiners and referees decided almost nothing.** In the 48 runs of the last two samples
  the referees were asked about failing cases 73 times and confirmed a case once. The cases they
  set aside were wrong where they were looked at (`ttlcache`: a case that expected a value at the
  very moment the text says the entry has expired; both referees quoted the sentence). So the
  referees did what they are for, and what Prosage gains on `confdialect` and `restyle` comes
  from the rest: nothing hurries the run, a reply may think longer, and the `ui` skill is
  switched on for `restyle`. `prosage+no-examiner` is the run that would say so in numbers.
- **In the first sample a third of the examiners were lost** (7 of the 20 runs looked at): the
  reply thought for its whole allowance and wrote no script. Asked again one level lower it did
  the same every time, so the retry is now made with thinking off. After that, none was lost
  that way.
- **On `confdialect` and `connpool` the worker never called `finish`**: it wrote and ran cases
  of its own for all sixty replies (15 and 12 minutes), so the examiners' cases were never run.
  They are now run with the first reply whose own checks pass. In the third sample the mean time
  fell from 215 s to 183 s and the requests rose from 13.8 to 19.3; one run each, so neither
  figure is settled.
- **Prosage's one loss is a verdict it threw away.** On `audit` the work lacked the file the
  task asks for (`findings.json`). All 17 cases of the examiner failed for that reason, and a
  script more than three in five of whose cases fail is taken to be at fault and set aside.
  Changed afterwards: with referees, such a script goes to them like any other.
- Six examiner replies ran into the output limit before the script ended and counted as "no
  script". Such a reply is now asked for again, shorter; not yet seen in a run.

### Changed after these runs, and not measured

Read from the logs of the third sample, where 14 of a run's 19 replies came after its own checks
had passed and four runs used 57 to 60 of their 60 replies:

- **A stopping rule** for a run nothing hurries: after 8 replies that end with the work passing
  everything it is told to finish or to name what is unmet, after 16 its next reply is its last.
  The runs that solved a task had 1 to 27 such replies, so the rule can cost a task that a long
  tail would have solved. `confdialect` (27) is the one to watch.
- **The router picks one skill**, for what the task asks to be delivered. It had added the
  `tests` skill to 11 of 29 bug fixes, and those runs wrote test suites and mutation loops.
- An examination that holds nothing is reported once, without the count of cases set aside: one
  worker went looking for those cases through the session's folders and printed the harness's
  key file, which `shell`, `read` and `search` now refuse.

A check of these on eight tasks was stopped after four runs (all solved), which says nothing.

### What limits these numbers

- Two runs per task in the head-to-head, one in each later sample, one model.
- The three Prosage rows are three versions of the code on the same day.
- Times are from a 2-core sandbox with four to six accounts busy, and Prosage's requests beside
  the run share the gateway's limit of 12 in flight with everything else.
- Not graded: solved means the held-out verification passed.

## Where Sage loses, read from the stored runs, 12 November 2026 (tenth session)

No run was made for this: the gateway was down. It is every results file under `docs/results/`
read again, runs lost to provider errors left out (`docs/results/2026-11-12-modes/overall.py` and
`by_variant.py` print the tables). DeepSeek V4.1 Flash:

| Variant | Solved | Lost after declaring the work done | Lost on |
|---|---|---|---|
| `ecosage` | 297/310 | 12 | `restyle` 8, `confdialect` 5 |
| `no-review` | 97/103 | 5 | `confdialect` 4, `restyle` 2 |
| `classic` (now `default`) | 92/94 | 0 | `deadcode-js` 1, `rollup` 1 |
| `ext:opencode` | 116/122 | 2 | `audit` 2, `audit-clean` 2, `restyle` 1, `nplusone` 1 |

On the two tasks alone, all of Sage's variants against the conventional loops (`classic`,
OpenCode, mini-swe-agent):

| Task | Sage's variants | Conventional loops |
|---|---|---|
| `confdialect` | 9/23, median 6 to 18 replies where it declared done | 8/8, 26 to 30 replies |
| `restyle` | 8/21 | 5/7 |

- Sage's losses are of one kind: work declared finished and verified that fails checks the
  model never ran. The conventional loop has none of that kind in 94 runs.
- The conventional loop's runs on these two tasks are few (`classic` 2 and 1), and most of its 94
  are on the easy set. It is a lead to follow, not a result.
- This session's five first replies on `confdialect` thought for 12,900 to 14,100 tokens; three
  hit the 14,000-token cap and were cut. Whether the cut costs the task is not known: of the two
  runs that finished, one had been cut and one had not, and both failed.

Prosage (`README.md`, "Modes") is built on this reading, was reworked in the eleventh session on
what others have measured about checks written by a second model (`docs/RESEARCH.md`, section 3),
and was first run against a model that evening (the section above).

## Against OpenCode, 12 November 2026 (ninth session)

Sage against OpenCode 1.19.5 (`ext:opencode`, its default `build` agent behind the metering
proxy), DeepSeek V4.1 Flash through Logfare, all 24 tasks, 30 replies and 15 minutes per run. Raw
results, the CLI's tables and the scripts that made the figures are in
`docs/results/2026-11-12-opencode/`.

The session began with a baseline of both harnesses as they stood, changed Sage where its own
logs showed waste, and ended with a second head-to-head in one time window. In that last one every
run had a Unix account to itself (three accounts per harness, one run at a time in each), so no run
could see or signal another, and the repository with its fixtures and reference solutions was
unreadable to all of them.

### Result

| 24 tasks | Solved | Requests (mean, median) | Time (mean, median) | Cost | Tokens per run, in / out |
|---|---|---:|---:|---:|---:|
| Sage as it is now, three runs per task | 67/72 | 5.7, 4 | 59 s, 39 s | $1.12 | 59k / 9.4k |
| OpenCode 1.19.5, three runs, same window | 70/72 | 10.8, 8 | 116 s, 50 s | $2.23 | 143k / 17.1k |
| Sage as it was that morning, two runs | 45/48 | 10.5, 10 | 107 s, 60 s | $1.22 | 148k / 17.1k |
| OpenCode that morning, two runs, same window | 44/48 | 11.0, 9 | 129 s, 48 s | $1.12 | 132k / 17.0k |

- **Sage now takes about half of what OpenCode takes**: 47% fewer requests, half the time and
  half the money, in each of the three repeats. That morning the two were level.
- **OpenCode solved three runs more of 72**, which by itself is within chance (p = 0.44), but the
  five runs Sage lost are on two tasks: `restyle` three times out of three and `confdialect`
  twice out of three, both solved by OpenCode every time. Over every DeepSeek run of the day,
  whatever the variant: `confdialect` 7/20 for Sage against 5/5 for OpenCode (p = 0.015),
  `restyle` 8/20 against 4/5 (p = 0.16), and the other 22 tasks 439/440 against 107/112.
  `confdialect` is failed on one or two of 39 hidden cases of a ten-section specification, after
  5 to 18 replies where OpenCode spends 25 to 30 requests and 13 to 15 minutes on it; `restyle` on
  the hidden check's spacing and type scale (15 to 20 spacing values where 8 are allowed). Neither
  is fixed.
- OpenCode's two failures: `nplusone` declared done and wrong, and `audit` killed by a signal after
  its shell tool timed out on a server the model had started in the background. That morning the
  same took two runs at once, while two OpenCode runs shared an account; run again alone, both
  solved, which would make the morning 46/48. Three of the morning's OpenCode runs also met a
  temporary directory they could not write to, left by the installation. Neither happened in the
  head-to-head of the first two rows.
- Cost is at list price and includes DeepSeek's doubling between 01:00 and 04:00 and between 06:00
  and 10:00 UTC, so it compares within one window only. OpenCode's requests include one title
  request per run.

### What was changed, and what each change is worth

Read off Sage's own session logs of the baseline (`review_cost.py`, `turns.py`), then measured.

| Change | Measured |
|---|---|
| **The review pass is off unless asked for** (`--with review`, `ecosage+review`). | Different windows: 45/48 with it at 10.5 replies, 45/48 without at 6.9. Same window, runs alternating: 46/48 at 9.5 replies and 77 s with it, 46/48 at 7.2 and 60 s without. The same tasks failed either way. No run of either kind left a scratch file behind; the review removed five `__pycache__` directories in 48 runs. |
| **`write` replaces a file the model has been shown in full** without `overwrite:true`. | Refused writes: 16 and 11 per 48 runs before, 1 to 4 after. Each refusal had also skipped the steps behind it in the same reply (36 in the baseline) and made the model send the whole file a second time. |
| **The watch stays on the test command the task names**, and a throwaway script (outside the workspace, or written on the spot with a here-document) never becomes the watch. | The watch was moved to another command 14 to 40 times per 48 runs before, never after. It had followed `node /tmp/verify-queue.mjs` and a mutation-testing loop in `/tmp` that is meant to fail, and `finish` was refused for it. |
| **The effort ratchet starts at `low`**, not `high`. | DeepSeek, same window: 46/48 at 7.7 replies, 77 s and 14.5k output tokens from `high`; 46/48 at 5.9 replies, 38 s and 8.3k from `low`. In the last head-to-head: 23/24 at 6.5 replies from `high`, 67/72 at 5.7 from `low`. GLM-5.3, six tasks, one run: 6/6 both, 15.5 replies and $1.04 from `high`, 7.3 and $0.36 from `low`. A fixed `low` with no ratchet solved 47/48 in one window and 43/48 in the next, and was not adopted. |
| **Thinking is not switched off at the first reply that only reasoned.** | Starting at `low`, the cut at 14,000 tokens of thinking took the effort to `off` for the rest of the run. The three `confdialect` runs it happened to used 29 to 30 replies and two failed. Now the first such reply leaves `low` as it is and the second switches thinking off. |
| **The metering proxy waits for a request in flight** before it closes a run. | A test of the bench failed in this sandbox whenever the adapter exited before its answer was counted; it passes now. No effect on results. |

### What limits these numbers

- Two or three runs per task. A difference of two or three solved runs in 72 is not a finding by
  itself; `confdialect` is, because it repeats.
- One model for the head-to-head. GLM-5.3 was run on six tasks, for the effort change only.
- The steps were measured one after another on the same day against the same gateway, whose speed
  varies: only rows of one window compare on time and cost.
- `restyle` was solved six times out of six in one 48-minute window (all three variants of that
  experiment) and twice out of 14 in the rest of the day. Nothing in the sessions explains it.
- The answers were not graded: solved means the held-out verification passed.
- About 5,600 of the gateway's 10,000 daily requests were used. The session logs of the runs
  were lost with the sandbox they were made in; the result files were saved first.

> **The tables below are from before the seventh session**, and so from before everything above:
> the last reply of the budget, the review that carries the diff, the anchor resolver and the tool
> fixes described in `docs/HANDOFF.md`, and classic being told its working directory.

## Skill and hard task sets, against other harnesses, 11 November 2026 (sixth session)

The seven tasks above are solved by every variant, so they measure turns only. Two harder sets were
added: `skill` (ten tasks, one per built-in skill: audits, dead-code removal, writing tests,
performance, a heisenbug, a restyle) and `hard` (seven tasks with several files and held-out checks
for behaviour the task specifies but the visible tests do not cover). Variants: `ecosage`, `classic`,
and mini-swe-agent 2.4.6 (`ext:mini`) driven through the same gateway behind a metering proxy.
30 replies and 15 minutes per run. Raw results, the CLI's tables, the grades and the scripts that
produced the figures below are in `docs/results/2026-11-11-session6/`.

**One run per task and variant.** The gateway key allows 12 requests at a time, 2,000 an hour and
10,000 a day; the daily limit ended the session before the GLM-5.3 run and the ablations were
complete. With one run per cell, a difference of one solved task is not a finding. Turns, time and
cost are steadier, and the before/after rows compare runs made under different load.

### DeepSeek V4.1 Flash: 17 tasks

| Variant | Solved | Turns (mean) | Time (mean) | Cost (17 runs) | Answer grade (of 8) |
|---|---|---:|---:|---:|---:|
| Sage | 16/17 | 14.3 | 128 s | $0.377 | 7.53 |
| Classic | 15/17 | 20.8 | 139 s | $0.323 | 7.06 |
| mini-swe-agent | 16/17 | 13.9 | 165 s | $0.372 | 7.24 |

- Sage against classic: 45% fewer turns (median 11 against 20), 35% less time, 13% lower cost at
  the median. Against mini-swe-agent: about the same number of turns and the same cost, a third
  less time.
- By set: on the ten skill tasks mini-swe-agent is the leanest (11.2 turns, $0.015 a run; Sage
  14.6 turns, $0.021). On the seven hard tasks Sage is (13.9 turns and 127 s; mini-swe-agent 17.7
  turns and 243 s; classic 19.0 turns).
- The unsolved runs: `restyle` under Sage and mini-swe-agent (both declared it done; the hidden
  check wants a spacing and type scale that only the `ui` skill spells out), `deadcode-js` and
  `rollup` under classic (turn limit).
- **Five of the 17 classic runs left their workspace** (`find /`, then work on whatever copy of the
  project they found, among them this repository's fixtures; one read a hidden verifier). No Sage
  run did: its first message says where the workspace is. Four of the five were run again with the
  fixtures removed from disk: all four solved (`deadcode-js` now in 12 turns), which would make
  classic 16/17; `rollup` could not be re-run before the daily limit.

### GLM-5.3: 6 of the 17 tasks

`audit`, `audit-clean`, `deadcode`, `shipquote`, `attempts`, `connpool`; the run stopped at the daily
limit.

| Variant | Solved | Turns (mean) | Time (mean) | Cost (mean) | Answer grade (of 8) |
|---|---|---:|---:|---:|---:|
| Sage (effort `high`) | 5/6 | 21.0 | 324 s | $0.305 | 7.33 |
| Sage, effort `max` | 6/6 | 20.5 | 448 s | $0.361 | not graded |
| Classic | 5/6 | 25.7 | 292 s | $0.229 | 5.83 |
| mini-swe-agent | 5/6 | 27.3 | 479 s | $0.431 | 6.17 |

Six tasks and one run each separate nothing on solve rate. mini-swe-agent ran into the 30-reply
limit on five of the six; it sends no effort setting, so GLM-5.3 thinks at its default there.

### Before and after this session's changes

| Model, tasks | Sage before | Sage after |
|---|---|---|
| DeepSeek, 10 skill tasks | 8/9 solved, 22.7 turns, 202 s, $0.034 | 9/10 solved, 14.6 turns, 129 s, $0.021 |
| DeepSeek, 7 hard tasks | 7/7 solved, 20.1 turns, 254 s, $0.033 | 7/7 solved, 13.9 turns, 127 s, $0.024 |
| GLM-5.3, `audit`, `audit-clean`, `deadcode` | 1/2 solved, 28.0 turns, 457 s | 3/3 solved, 18.3 turns, 240 s |
| GLM-5.3, 10 skill tasks | 6/9 solved, 22.1 turns, 437 s | not run (daily limit) |

"Before" is the engine as merged in PR #4 for the skill tasks, and with only the finish-gate fix
for the hard tasks. The before runs shared the gateway with more parallel runs, which lengthens
time but not turns. What changed in between, found by reading the traces of the before runs:

- **Finish gate.** A check written as `cd dir && <check>` did not count, so `finish` was refused
  until the turn limit (five refusals in one run).
- **GLM-5.3 effort.** Logfare passes GLM-5.3 only a top-level `reasoning_effort` of `low`, `high` or
  `max`; with `reasoning: {effort}`, which the harness sent, it thinks at `max`. Measured on one
  prompt: about 6,000 reasoning tokens and no answer within the limit, against 90 to 960 with
  `reasoning_effort: high`. In the before runs GLM-5.3 filled whole 32,000-token replies with
  reasoning, three times in a row, without a tool call (about 200 s each). Both spellings are now
  sent, and a reply that only reasons is cut at about 14,000 tokens and asked for again one level
  lower.
- **Survey, budget, single-call streaks.** A small workspace is listed and attached up front; the
  budget is in the first message; a streak of single-call replies is pointed out.
- A reply cut off by the output limit, or a plan in prose, ended an unattended run as its answer.

### Ablations on DeepSeek (10 of 17 tasks each, one run; stopped by the daily limit)

| Variant | Solved | Turns | against Sage on the same tasks |
|---|---|---:|---|
| `no-review` | 9/10 | 13.6 | 9/10, 15.5 turns |
| `no-survey` | 9/10 | 17.6 | 9/10, 17.5 turns |
| `ecosage+skill` (8 skill tasks) | 8/8 | 16.2 | 7/8, 12.1 turns |

- **Review pass**: costs about two turns and changed no outcome here. Undecided; ten runs cannot
  show a rare rescue.
- **Survey**: no difference in turns on these ten tasks. The drop from 22 to 14 turns above is
  therefore not shown to come from it; the ablation does not isolate the gate fix or the budget
  line, which have no switch.
- **Skills**: the one task Sage fails without a skill (`restyle`) is solved with `ui`. Elsewhere
  the skill costs turns (16.2 against 12.1) and about twice the money, for no change in outcome.

### Grading the answers

Each DeepSeek run and the graded GLM-5.3 runs were exported as a bundle (task, hidden verification
result, final message, diff) and scored 0 to 2 on four counts: correct, scope, honest report,
quality (`grading-rubric.md`). Claude Haiku 5.5 graded every bundle; Claude Fable 5.2 then re-graded
36 of the 51 DeepSeek bundles and all GLM-5.3 bundles from the bundle itself and corrected the file.
On DeepSeek it changed 15 scores in 12 bundles, mostly `honest`, in both directions and without a
lean towards one variant.

| DeepSeek | correct | scope | honest | quality | total |
|---|---:|---:|---:|---:|---:|
| Sage | 1.94 | 2.00 | 1.65 | 1.94 | 7.53 |
| mini-swe-agent | 1.94 | 2.00 | 1.65 | 1.65 | 7.24 |
| Classic | 1.82 | 2.00 | 1.47 | 1.76 | 7.06 |

Sage's lead is 5 points of 136; second against third is 3 points and could swap with a repeat.
On the six GLM-5.3 tasks the totals are Sage 7.33, mini-swe-agent 6.17, classic 5.83, where one
run that stops at its limit moves a mean by half a point.
Every variant loses most on `honest`: runs that stop at the turn limit leave no final message.

### What makes these numbers weaker than they look

- One run per cell, and the two models ran different numbers of tasks.
- **Runs share one machine.** They see each other's scratch files in `/tmp` and each other's
  workspaces, and an agent's `pkill -f <word of the task>` killed mini-swe-agent and opencode
  processes of other runs, whose command line held the task. The mini-swe-agent adapter now passes
  the task through the environment (opencode's still takes it on the command line), the bench makes
  each workspace its own git repository, and the fixtures were removed from disk for the later
  runs, but there is no isolation between runs. A run
  that leaves its workspace is found afterwards by `wander.py`, not prevented.
- opencode was only smoke-tested (two easy tasks, both solved) and is not in the tables.

## Baseline, 10 November 2026 (engine at d739e56, before `attach` and `short_paths`)

### GLM-5.3: 140 runs, all solved, 20 minutes, $2.46

| Variant | Solved | Turns: median (mean, range) | Input tok | Cached | Output tok | Cost | No cache | Mean cost | Time |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| sage | 35/35 | 3 (3.6, 2–9) | 8.8k | 58% | 395 | $0.0077 | $0.0141 | $0.0134 | 6.4 s |
| no-preflight | 35/35 | 4 (4.3, 3–7) | 8.9k | 63% | 442 | $0.0087 | $0.0142 | $0.0157 | 9.3 s |
| no-auto-watch | 35/35 | 4 (4.8, 2–13) | 9.4k | 73% | 439 | $0.0084 | $0.0151 | $0.0166 | 8.8 s |
| classic | 35/35 | 6 (7.4, 4–27) | 9.9k | 66% | 415 | $0.0088 | $0.0154 | $0.0247 | 9.8 s |

Median turns and median cost per task:

| Task | sage | no-preflight | no-auto-watch | classic |
|---|---|---|---|---|
| pager | 2 · $0.0062 | 4 · $0.0066 | 4 · $0.0063 | 5 · $0.0067 |
| inventory | 3 · $0.0068 | 4 · $0.0088 | 4 · $0.0085 | 5 · $0.0087 |
| slug | 3 · $0.0076 | 4 · $0.0086 | 4 · $0.0063 | 5 · $0.0082 |
| tariff | 3 · $0.0107 | 4 · $0.0082 | 5 · $0.0103 | 6 · $0.0177 |
| textstats | 3 · $0.0068 | 4 · $0.0066 | 4 · $0.0075 | 6 · $0.0071 |
| roman | 8 · $0.0383 | 6 · $0.0398 | 5 · $0.0412 | 8 · $0.0469 |
| report | 3 · $0.0063 | 5 · $0.0098 | 6 · $0.0089 | 8 · $0.0197 |

### DeepSeek V4.1 Flash (`deepseek-v4.1-flash-1017`): 140 runs, all solved, 7 minutes, $0.16

| Variant | Solved | Turns: median (mean, range) | Input tok | Cached | Output tok | Cost | No cache | Mean cost | Time |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| sage | 35/35 | 3 (3.6, 2–12) | 9.6k | 63% | 442 | $0.0008 | $0.0017 | $0.00124 | 3.8 s |
| no-preflight | 35/35 | 4 (4.3, 3–8) | 9.7k | 71% | 618 | $0.0009 | $0.0019 | $0.00113 | 4.8 s |
| no-auto-watch | 35/35 | 4 (4.3, 3–8) | 9.5k | 70% | 619 | $0.0010 | $0.0020 | $0.00123 | 4.9 s |
| classic | 35/35 | 6 (5.6, 3–8) | 10.4k | 65% | 639 | $0.0009 | $0.0019 | $0.00106 | 5.4 s |

| Task | sage | no-preflight | no-auto-watch | classic |
|---|---|---|---|---|
| pager | 3 · $0.0005 | 3 · $0.0005 | 4 · $0.0010 | 5 · $0.0011 |
| inventory | 3 · $0.0008 | 4 · $0.0009 | 3 · $0.0007 | 5 · $0.0007 |
| slug | 3 · $0.0008 | 3 · $0.0006 | 3 · $0.0007 | 5 · $0.0007 |
| tariff | 3 · $0.0014 | 6 · $0.0012 | 4 · $0.0018 | 5 · $0.0013 |
| textstats | 3 · $0.0008 | 4 · $0.0008 | 4 · $0.0008 | 6 · $0.0008 |
| roman | 4 · $0.0022 | 4 · $0.0017 | 4 · $0.0016 | 6 · $0.0018 |
| report | 4 · $0.0006 | 6 · $0.0014 | 4 · $0.0009 | 5 · $0.0009 |

### What the baseline shows

- **Turns halve.** Median 3 against 6 on both models. Fewer on 6 of 7 tasks on GLM-5.3 (`roman` ties
  at 8) and on 7 of 7 on DeepSeek. Both ablations cost one turn: preflight and auto-watch each carry
  part of the saving.
- **Cost on GLM-5.3: about a tenth lower in a typical run.** Median −13% billed, −8% with no cache.
  Total spend is 46% lower ($0.469 against $0.864), but nearly all of that gap is two runaway
  classic runs on `report` (25 and 27 turns). Without `report` the mean saving is 9%.
- **Cost on DeepSeek: no saving.** Median −3% billed (−11% with no cache), mean +17% ($0.00124
  against $0.00106). Three Sage turns carry about as many input tokens as five or six classic
  ones: the fixed prefix is 1,193 estimated tokens against 762, the first message carries the
  preflight output, and reads carry anchors. One 12-turn `roman` run does the rest.
- **Time**: median −35% on GLM-5.3, −30% on DeepSeek.
- **Nothing can be said about reliability.** Every run of every variant was solved, so the tasks
  are too easy to separate the variants. The evidence gate verified 35 of 35 Sage runs; classic
  ends on the model's word and was right every time here.
- **Where the turns go.** A Sage run is almost always `read → edit → finish`. `finish` came in
  the same reply as the edits in 6 of 35 runs on GLM-5.3 and 1 of 35 on DeepSeek. With the watch
  doing the checking the model makes few predictions of its own (11 in 35 runs on GLM-5.3, 14 on
  DeepSeek, against about 50 without auto-watch): the saving comes from the harness running the
  check, not from predictions and receipts.
- **Small sample.** Five runs per cell. `roman` varies widely (GLM-5.3: Sage 4–9 turns,
  no-auto-watch 2–13); a difference on a single task is not established.

## Follow-up, 11 November 2026: `attach` and `short_paths`

`attach` puts the files the task names in the first message, which is aimed at the `read` turn
above. Variants `ecosage` (both new mechanisms on), `no-attach`, `no-short-paths`, `classic`; same
tasks, 5 runs each. The two models ran at the same time (GLM-5.3 with `-j 4`, DeepSeek with `-j 2`)
under Logfare's shared limit of 80 requests a minute, so the time column is not comparable with the
baseline.

The session ended before these runs did. What exists in `docs/results/`:

- `2026-11-11-*-attach.progress.txt`: one line per completed run (task, variant, solved, turns,
  cost). `-attach.txt` and `-attach.json` are the CLI's table and the raw results of a run that
  finished in time; an empty or missing file means it did not.
- GLM-5.3 after 55 of 140 runs, turns per run: `ecosage` 1,1,1,1,1,1,2,2,2,2,2,2,3,3 (median 2),
  `no-short-paths` median 2, `no-attach` median 3.5, `classic` median 6. A `pager` run with the file
  attached took 1 turn and $0.0032, against 3 turns and $0.0082 without. One classic run on `slug`
  declared itself done and failed the held-out check: the first unsolved run in any benchmark here.
- DeepSeek, the 42 runs of a first attempt that died (it hit the rate limit, then its process was
  gone): `ecosage` median 3 (1 to 9), `no-short-paths` 2, `no-attach` 3.5, `classic` 5.

The cells are unbalanced across tasks, so these medians are indicative only: attach appears to
remove the read turn, and nothing is established about `short_paths` or about cost. Repeat with one
model at a time:

```sh
sage bench -p logfare -m glm-5.3 --variants ecosage,no-attach,no-short-paths,classic -n 5 -j 3
```

### Update: runs that finished after the lines above were written

DeepSeek V4.1 Flash (`-j 2`, alongside the GLM-5.3 run), the table printed by the CLI:

```text
sage bench 20261111-000852-dd13 · logfare · effort ratchet · 5 runs per task and variant
fixed prompt prefix, estimated: sage ~1193 tokens, no-attach ~1193 tokens, no-short-paths ~1193 tokens, classic ~762 tokens

deepseek-v4.1-flash-1017   medians per run; solved = the held-out verification passed after the run; no cache = cost had the provider cached nothing
task       variant          solved turns  in tok cached out tok      cost  no cache    time
pager      sage             5/5     2    5.4k    52%     275   $0.0005   $0.0010    2.6s
           no-attach           5/5     3    7.8k    67%     316   $0.0006   $0.0014    3.3s
           no-short-paths      5/5     2    5.6k    71%     313   $0.0004   $0.0010    3.5s
           classic             5/5     5    8.6k    82%     551   $0.0007   $0.0017    6.5s
inventory  sage             5/5     3    9.3k    62%     477   $0.0008   $0.0017    4.1s
           no-attach           5/5     3    9.2k    71%     447   $0.0008   $0.0017    4.8s
           no-short-paths      5/5     2    6.2k    68%     382   $0.0007   $0.0012    5.8s
           classic             5/5     6   12.3k    76%     854   $0.0013   $0.0024    6.1s
slug       sage             5/5     1    2.3k    60%     397   $0.0004   $0.0006    2.7s
           no-attach           5/5     4   10.9k    69%     456   $0.0008   $0.0020    4.5s
           no-short-paths      5/5     2    5.1k    55%     449   $0.0005   $0.0010    2.9s
           classic             5/5     5    8.2k    77%     611   $0.0007   $0.0016    5.3s
tariff     sage             5/5     2    8.8k    63%     383   $0.0008   $0.0016    3.1s
           no-attach           5/5     4   16.9k    68%     387   $0.0012   $0.0028    7.8s
           no-short-paths      5/5     2    8.7k    63%     352   $0.0007   $0.0015    2.9s
           classic             5/5     7   15.9k    66%     663   $0.0012   $0.0028     20s
textstats  sage             5/5     3    8.2k    67%     310   $0.0006   $0.0014    4.6s
           no-attach           5/5     3    8.0k    69%     377   $0.0007   $0.0014    5.8s
           no-short-paths      5/5     3    8.4k    76%     453   $0.0007   $0.0015    4.2s
           classic             5/5     5    8.7k    84%     512   $0.0006   $0.0016    5.3s
roman      sage             5/5     4   10.0k    75%    1.7k   $0.0016   $0.0025    8.3s
           no-attach           5/5     4    9.1k    76%    1.6k   $0.0016   $0.0023    8.8s
           no-short-paths      5/5     5   13.7k    81%    2.3k   $0.0019   $0.0034     10s
           classic             5/5     9   20.9k    77%    3.5k   $0.0029   $0.0053     21s
report     sage             5/5     2    4.7k    77%     281   $0.0003   $0.0009    2.3s
           no-attach           5/5     3    6.8k    81%     465   $0.0005   $0.0013    4.1s
           no-short-paths      5/5     2    4.7k    60%     275   $0.0005   $0.0009    8.5s
           classic             5/5    20   66.2k    76%    5.1k   $0.0056   $0.0130     38s
ALL        sage           35/35     2    8.2k    45%     373   $0.0006   $0.0014    3.3s   total $0.027, $0.0008 per solved task
           no-attach         35/35     4    9.1k    67%     447   $0.0008   $0.0019    5.1s   total $0.033, $0.0009 per solved task
           no-short-paths    35/35     2    8.4k    61%     375   $0.0007   $0.0015    4.2s   total $0.030, $0.0009 per solved task
           classic           35/35     6   12.3k    74%     728   $0.0011   $0.0023    8.7s   total $0.072, $0.0021 per solved task
sage against classic: turns -67%, input tokens -33%, output tokens -49%, cost -42% (-39% with no cache), time -62%; solved 35/35 against 35/35
  by task (7): fewer turns on 7, more on 0; cheaper on 7, dearer on 0 (with no cache: 7 and 0)
no-attach against classic: turns -33%, input tokens -26%, output tokens -39%, cost -22% (-20% with no cache), time -41%; solved 35/35 against 35/35
  by task (7): fewer turns on 7, more on 0; cheaper on 4, dearer on 3 (with no cache: 6 and 1)
no-short-paths against classic: turns -67%, input tokens -32%, output tokens -48%, cost -33% (-38% with no cache), time -51%; solved 35/35 against 35/35
  by task (7): fewer turns on 7, more on 0; cheaper on 6, dearer on 1 (with no cache: 7 and 0)

```

GLM-5.3 (`-j 4`, alongside the DeepSeek run), the table printed by the CLI:

```text
sage bench 20261111-000429-208a · logfare · effort ratchet · 5 runs per task and variant
fixed prompt prefix, estimated: sage ~1193 tokens, no-attach ~1193 tokens, no-short-paths ~1193 tokens, classic ~762 tokens

glm-5.3   medians per run; solved = the held-out verification passed after the run; no cache = cost had the provider cached nothing
task       variant          solved turns  in tok cached out tok      cost  no cache    time
pager      sage             5/5     1    2.3k    52%     222   $0.0029   $0.0042    2.8s
           no-attach           5/5     3    7.1k    64%     226   $0.0058   $0.0109    4.2s
           no-short-paths      5/5     1    2.4k    50%     265   $0.0038   $0.0052    4.2s
           classic             5/5     5    8.2k    66%     281   $0.0066   $0.0128    8.8s
inventory  sage             5/5     3    8.4k    74%     511   $0.0074   $0.0140    6.2s
           no-attach           5/5     4   10.9k    56%     514   $0.0078   $0.0172    6.8s
           no-short-paths      5/5     3    8.9k    59%     780   $0.0099   $0.0160    8.2s
           classic             5/5     5    9.8k    66%     372   $0.0074   $0.0154    7.6s
slug       sage             5/5     2    4.8k    68%    1.3k   $0.0109   $0.0131     12s
           no-attach           5/5     3    7.1k    77%     584   $0.0066   $0.0117     12s
           no-short-paths      5/5     2    4.9k    67%    1.1k   $0.0079   $0.0112     15s
           classic             4/5     5    7.7k    71%     390   $0.0063   $0.0125     14s   1 declared done, then failed verification
tariff     sage             5/5     2    8.4k    61%     504   $0.0082   $0.0140    8.0s
           no-attach           5/5     4   16.3k    77%     498   $0.0144   $0.0250     11s
           no-short-paths      5/5     1    4.0k    31%    1.2k   $0.0101   $0.0115     10s
           classic             5/5     6   13.6k    58%     402   $0.0118   $0.0207     13s
textstats  sage             5/5     3    7.7k    59%    1.6k   $0.0101   $0.0149     16s
           no-attach           5/5     5   13.8k    48%     553   $0.0112   $0.0215    9.7s
           no-short-paths      5/5     3    8.2k    56%     637   $0.0091   $0.0143    9.0s
           classic             5/5     7   11.4k    68%     513   $0.0084   $0.0179     14s
roman      sage             5/5     2    5.6k    48%   17.6k   $0.0847   $0.0877   2m09s
           no-attach           5/5     7   21.7k    46%   21.7k   $0.1103   $0.1413   2m34s
           no-short-paths      5/5     6   20.3k    63%   11.4k   $0.0672   $0.0813   1m53s
           classic             5/5     9   29.3k    63%   11.2k   $0.0588   $0.0676   1m40s
report     sage             5/5     2    4.3k    73%     423   $0.0045   $0.0079    4.6s
           no-attach           5/5     3    6.0k    72%     435   $0.0054   $0.0103    6.9s
           no-short-paths      5/5     2    4.3k    57%     561   $0.0055   $0.0083     12s
           classic             5/5     6    8.5k    60%     545   $0.0076   $0.0145     14s
ALL        sage           35/35     2    4.8k    65%     955   $0.0084   $0.0131     11s   total $0.699, $0.0200 per solved task
           no-attach         35/35     3    9.4k    64%     514   $0.0089   $0.0172    9.7s   total $0.859, $0.0245 per solved task
           no-short-paths    35/35     2    6.1k    58%     827   $0.0086   $0.0127     11s   total $0.735, $0.0210 per solved task
           classic           34/35     6    9.9k    66%     477   $0.0084   $0.0154     14s   total $0.791, $0.0233 per solved task   1 declared done, then failed verification
sage against classic: turns -67%, input tokens -51%, output tokens +100%, cost -0% (-15% with no cache), time -22%; solved 35/35 against 34/35
  by task (7): fewer turns on 7, more on 0; cheaper on 4, dearer on 3 (with no cache: 5 and 2)
no-attach against classic: turns -50%, input tokens -5%, output tokens +8%, cost +5% (+12% with no cache), time -31%; solved 35/35 against 34/35
  by task (7): fewer turns on 7, more on 0; cheaper on 2, dearer on 5 (with no cache: 3 and 4)
no-short-paths against classic: turns -67%, input tokens -38%, output tokens +73%, cost +2% (-17% with no cache), time -16%; solved 35/35 against 34/35
  by task (7): fewer turns on 7, more on 0; cheaper on 3, dearer on 4 (with no cache: 5 and 2)

1 run without a solution:
  glm-5.3 · slug · classic #1 · session 20261111-000501-8b97 · done; verification: FAILED (failures=2)

```
