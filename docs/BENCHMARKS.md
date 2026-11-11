# Benchmarks

`presage bench` runs 7 small synthetic tasks (Python and shell: fix bugs, implement a function from
a README, write a module that has no tests) under several harness variants. Every run starts from a
fresh copy of the task. When the agent is done, the files it was told to leave alone are put back,
held-out checks are added and the task's verification is run. Only that counts as solved.
`presage bench --check` proves that each task fails as shipped and passes with its reference
solution (`--tasks all` for all 24; without it, the seven of the `easy` set).

All runs below: Logfare gateway, effort `ratchet`, 5 runs per task and variant (35 per variant).
Figures are medians per run unless stated. "No cache" is the list-price cost had the provider
cached nothing, which takes the luck of the cache out of a comparison. Raw results and the CLI's own
tables are in `docs/results/`.

```sh
cargo build
target/debug/presage bench --check --tasks all
LOGFARE_API_KEY=... PRESAGE_HOME=/tmp/presage-live target/debug/presage bench -p logfare -m glm-5.3 \
  --variants presage,no-preflight,no-auto-watch,classic -n 5 -j 3 --out /tmp/glm-5.3.json
```

> **These numbers are from before the seventh session.** The last reply of the budget, the review
> that carries the diff, the anchor resolver and the tool fixes described in `docs/HANDOFF.md` were
> made without a benchmark run, and classic is now told its working directory. Re-run before
> comparing anything with the tables below.

## Skill and hard task sets, against other harnesses, 11 November 2026 (sixth session)

The seven tasks above are solved by every variant, so they measure turns only. Two harder sets were
added: `skill` (ten tasks, one per built-in skill: audits, dead-code removal, writing tests,
performance, a heisenbug, a restyle) and `hard` (seven tasks with several files and held-out checks
for behaviour the task specifies but the visible tests do not cover). Variants: `presage`, `classic`,
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
| Presage | 16/17 | 14.3 | 128 s | $0.377 | 7.53 |
| Classic | 15/17 | 20.8 | 139 s | $0.323 | 7.06 |
| mini-swe-agent | 16/17 | 13.9 | 165 s | $0.372 | 7.24 |

- Presage against classic: 45% fewer turns (median 11 against 20), 35% less time, 13% lower cost at
  the median. Against mini-swe-agent: about the same number of turns and the same cost, a third
  less time.
- By set: on the ten skill tasks mini-swe-agent is the leanest (11.2 turns, $0.015 a run; Presage
  14.6 turns, $0.021). On the seven hard tasks Presage is (13.9 turns and 127 s; mini-swe-agent 17.7
  turns and 243 s; classic 19.0 turns).
- The unsolved runs: `restyle` under Presage and mini-swe-agent (both declared it done; the hidden
  check wants a spacing and type scale that only the `ui` skill spells out), `deadcode-js` and
  `rollup` under classic (turn limit).
- **Five of the 17 classic runs left their workspace** (`find /`, then work on whatever copy of the
  project they found, among them this repository's fixtures; one read a hidden verifier). No Presage
  run did: its first message says where the workspace is. Four of the five were run again with the
  fixtures removed from disk: all four solved (`deadcode-js` now in 12 turns), which would make
  classic 16/17; `rollup` could not be re-run before the daily limit.

### GLM-5.3: 6 of the 17 tasks

`audit`, `audit-clean`, `deadcode`, `shipquote`, `attempts`, `connpool`; the run stopped at the daily
limit.

| Variant | Solved | Turns (mean) | Time (mean) | Cost (mean) | Answer grade (of 8) |
|---|---|---:|---:|---:|---:|
| Presage (effort `high`) | 5/6 | 21.0 | 324 s | $0.305 | 7.33 |
| Presage, effort `max` | 6/6 | 20.5 | 448 s | $0.361 | not graded |
| Classic | 5/6 | 25.7 | 292 s | $0.229 | 5.83 |
| mini-swe-agent | 5/6 | 27.3 | 479 s | $0.431 | 6.17 |

Six tasks and one run each separate nothing on solve rate. mini-swe-agent ran into the 30-reply
limit on five of the six; it sends no effort setting, so GLM-5.3 thinks at its default there.

### Before and after this session's changes

| Model, tasks | Presage before | Presage after |
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

| Variant | Solved | Turns | against Presage on the same tasks |
|---|---|---:|---|
| `no-review` | 9/10 | 13.6 | 9/10, 15.5 turns |
| `no-survey` | 9/10 | 17.6 | 9/10, 17.5 turns |
| `presage+skill` (8 skill tasks) | 8/8 | 16.2 | 7/8, 12.1 turns |

- **Review pass**: costs about two turns and changed no outcome here. Undecided; ten runs cannot
  show a rare rescue.
- **Survey**: no difference in turns on these ten tasks. The drop from 22 to 14 turns above is
  therefore not shown to come from it; the ablation does not isolate the gate fix or the budget
  line, which have no switch.
- **Skills**: the one task Presage fails without a skill (`restyle`) is solved with `ui`. Elsewhere
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
| Presage | 1.94 | 2.00 | 1.65 | 1.94 | 7.53 |
| mini-swe-agent | 1.94 | 2.00 | 1.65 | 1.65 | 7.24 |
| Classic | 1.82 | 2.00 | 1.47 | 1.76 | 7.06 |

Presage's lead is 5 points of 136; second against third is 3 points and could swap with a repeat.
On the six GLM-5.3 tasks the totals are Presage 7.33, mini-swe-agent 6.17, classic 5.83, where one
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
| presage | 35/35 | 3 (3.6, 2–9) | 8.8k | 58% | 395 | $0.0077 | $0.0141 | $0.0134 | 6.4 s |
| no-preflight | 35/35 | 4 (4.3, 3–7) | 8.9k | 63% | 442 | $0.0087 | $0.0142 | $0.0157 | 9.3 s |
| no-auto-watch | 35/35 | 4 (4.8, 2–13) | 9.4k | 73% | 439 | $0.0084 | $0.0151 | $0.0166 | 8.8 s |
| classic | 35/35 | 6 (7.4, 4–27) | 9.9k | 66% | 415 | $0.0088 | $0.0154 | $0.0247 | 9.8 s |

Median turns and median cost per task:

| Task | presage | no-preflight | no-auto-watch | classic |
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
| presage | 35/35 | 3 (3.6, 2–12) | 9.6k | 63% | 442 | $0.0008 | $0.0017 | $0.00124 | 3.8 s |
| no-preflight | 35/35 | 4 (4.3, 3–8) | 9.7k | 71% | 618 | $0.0009 | $0.0019 | $0.00113 | 4.8 s |
| no-auto-watch | 35/35 | 4 (4.3, 3–8) | 9.5k | 70% | 619 | $0.0010 | $0.0020 | $0.00123 | 4.9 s |
| classic | 35/35 | 6 (5.6, 3–8) | 10.4k | 65% | 639 | $0.0009 | $0.0019 | $0.00106 | 5.4 s |

| Task | presage | no-preflight | no-auto-watch | classic |
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
  against $0.00106). Three Presage turns carry about as many input tokens as five or six classic
  ones: the fixed prefix is 1,193 estimated tokens against 762, the first message carries the
  preflight output, and reads carry anchors. One 12-turn `roman` run does the rest.
- **Time**: median −35% on GLM-5.3, −30% on DeepSeek.
- **Nothing can be said about reliability.** Every run of every variant was solved, so the tasks
  are too easy to separate the variants. The evidence gate verified 35 of 35 Presage runs; classic
  ends on the model's word and was right every time here.
- **Where the turns go.** A Presage run is almost always `read → edit → finish`. `finish` came in
  the same reply as the edits in 6 of 35 runs on GLM-5.3 and 1 of 35 on DeepSeek. With the watch
  doing the checking the model makes few predictions of its own (11 in 35 runs on GLM-5.3, 14 on
  DeepSeek, against about 50 without auto-watch): the saving comes from the harness running the
  check, not from predictions and receipts.
- **Small sample.** Five runs per cell. `roman` varies widely (GLM-5.3: Presage 4–9 turns,
  no-auto-watch 2–13); a difference on a single task is not established.

## Follow-up, 11 November 2026: `attach` and `short_paths`

`attach` puts the files the task names in the first message, which is aimed at the `read` turn
above. Variants `presage` (both new mechanisms on), `no-attach`, `no-short-paths`, `classic`; same
tasks, 5 runs each. The two models ran at the same time (GLM-5.3 with `-j 4`, DeepSeek with `-j 2`)
under Logfare's shared limit of 80 requests a minute, so the time column is not comparable with the
baseline.

The session ended before these runs did. What exists in `docs/results/`:

- `2026-11-11-*-attach.progress.txt`: one line per completed run (task, variant, solved, turns,
  cost). `-attach.txt` and `-attach.json` are the CLI's table and the raw results of a run that
  finished in time; an empty or missing file means it did not.
- GLM-5.3 after 55 of 140 runs, turns per run: `presage` 1,1,1,1,1,1,2,2,2,2,2,2,3,3 (median 2),
  `no-short-paths` median 2, `no-attach` median 3.5, `classic` median 6. A `pager` run with the file
  attached took 1 turn and $0.0032, against 3 turns and $0.0082 without. One classic run on `slug`
  declared itself done and failed the held-out check: the first unsolved run in any benchmark here.
- DeepSeek, the 42 runs of a first attempt that died (it hit the rate limit, then its process was
  gone): `presage` median 3 (1 to 9), `no-short-paths` 2, `no-attach` 3.5, `classic` 5.

The cells are unbalanced across tasks, so these medians are indicative only: attach appears to
remove the read turn, and nothing is established about `short_paths` or about cost. Repeat with one
model at a time:

```sh
presage bench -p logfare -m glm-5.3 --variants presage,no-attach,no-short-paths,classic -n 5 -j 3
```

### Update: runs that finished after the lines above were written

DeepSeek V4.1 Flash (`-j 2`, alongside the GLM-5.3 run), the table printed by the CLI:

```text
presage bench 20261111-000852-dd13 · logfare · effort ratchet · 5 runs per task and variant
fixed prompt prefix, estimated: presage ~1193 tokens, no-attach ~1193 tokens, no-short-paths ~1193 tokens, classic ~762 tokens

deepseek-v4.1-flash-1017   medians per run; solved = the held-out verification passed after the run; no cache = cost had the provider cached nothing
task       variant          solved turns  in tok cached out tok      cost  no cache    time
pager      presage             5/5     2    5.4k    52%     275   $0.0005   $0.0010    2.6s
           no-attach           5/5     3    7.8k    67%     316   $0.0006   $0.0014    3.3s
           no-short-paths      5/5     2    5.6k    71%     313   $0.0004   $0.0010    3.5s
           classic             5/5     5    8.6k    82%     551   $0.0007   $0.0017    6.5s
inventory  presage             5/5     3    9.3k    62%     477   $0.0008   $0.0017    4.1s
           no-attach           5/5     3    9.2k    71%     447   $0.0008   $0.0017    4.8s
           no-short-paths      5/5     2    6.2k    68%     382   $0.0007   $0.0012    5.8s
           classic             5/5     6   12.3k    76%     854   $0.0013   $0.0024    6.1s
slug       presage             5/5     1    2.3k    60%     397   $0.0004   $0.0006    2.7s
           no-attach           5/5     4   10.9k    69%     456   $0.0008   $0.0020    4.5s
           no-short-paths      5/5     2    5.1k    55%     449   $0.0005   $0.0010    2.9s
           classic             5/5     5    8.2k    77%     611   $0.0007   $0.0016    5.3s
tariff     presage             5/5     2    8.8k    63%     383   $0.0008   $0.0016    3.1s
           no-attach           5/5     4   16.9k    68%     387   $0.0012   $0.0028    7.8s
           no-short-paths      5/5     2    8.7k    63%     352   $0.0007   $0.0015    2.9s
           classic             5/5     7   15.9k    66%     663   $0.0012   $0.0028     20s
textstats  presage             5/5     3    8.2k    67%     310   $0.0006   $0.0014    4.6s
           no-attach           5/5     3    8.0k    69%     377   $0.0007   $0.0014    5.8s
           no-short-paths      5/5     3    8.4k    76%     453   $0.0007   $0.0015    4.2s
           classic             5/5     5    8.7k    84%     512   $0.0006   $0.0016    5.3s
roman      presage             5/5     4   10.0k    75%    1.7k   $0.0016   $0.0025    8.3s
           no-attach           5/5     4    9.1k    76%    1.6k   $0.0016   $0.0023    8.8s
           no-short-paths      5/5     5   13.7k    81%    2.3k   $0.0019   $0.0034     10s
           classic             5/5     9   20.9k    77%    3.5k   $0.0029   $0.0053     21s
report     presage             5/5     2    4.7k    77%     281   $0.0003   $0.0009    2.3s
           no-attach           5/5     3    6.8k    81%     465   $0.0005   $0.0013    4.1s
           no-short-paths      5/5     2    4.7k    60%     275   $0.0005   $0.0009    8.5s
           classic             5/5    20   66.2k    76%    5.1k   $0.0056   $0.0130     38s
ALL        presage           35/35     2    8.2k    45%     373   $0.0006   $0.0014    3.3s   total $0.027, $0.0008 per solved task
           no-attach         35/35     4    9.1k    67%     447   $0.0008   $0.0019    5.1s   total $0.033, $0.0009 per solved task
           no-short-paths    35/35     2    8.4k    61%     375   $0.0007   $0.0015    4.2s   total $0.030, $0.0009 per solved task
           classic           35/35     6   12.3k    74%     728   $0.0011   $0.0023    8.7s   total $0.072, $0.0021 per solved task
presage against classic: turns -67%, input tokens -33%, output tokens -49%, cost -42% (-39% with no cache), time -62%; solved 35/35 against 35/35
  by task (7): fewer turns on 7, more on 0; cheaper on 7, dearer on 0 (with no cache: 7 and 0)
no-attach against classic: turns -33%, input tokens -26%, output tokens -39%, cost -22% (-20% with no cache), time -41%; solved 35/35 against 35/35
  by task (7): fewer turns on 7, more on 0; cheaper on 4, dearer on 3 (with no cache: 6 and 1)
no-short-paths against classic: turns -67%, input tokens -32%, output tokens -48%, cost -33% (-38% with no cache), time -51%; solved 35/35 against 35/35
  by task (7): fewer turns on 7, more on 0; cheaper on 6, dearer on 1 (with no cache: 7 and 0)

```

GLM-5.3 (`-j 4`, alongside the DeepSeek run), the table printed by the CLI:

```text
presage bench 20261111-000429-208a · logfare · effort ratchet · 5 runs per task and variant
fixed prompt prefix, estimated: presage ~1193 tokens, no-attach ~1193 tokens, no-short-paths ~1193 tokens, classic ~762 tokens

glm-5.3   medians per run; solved = the held-out verification passed after the run; no cache = cost had the provider cached nothing
task       variant          solved turns  in tok cached out tok      cost  no cache    time
pager      presage             5/5     1    2.3k    52%     222   $0.0029   $0.0042    2.8s
           no-attach           5/5     3    7.1k    64%     226   $0.0058   $0.0109    4.2s
           no-short-paths      5/5     1    2.4k    50%     265   $0.0038   $0.0052    4.2s
           classic             5/5     5    8.2k    66%     281   $0.0066   $0.0128    8.8s
inventory  presage             5/5     3    8.4k    74%     511   $0.0074   $0.0140    6.2s
           no-attach           5/5     4   10.9k    56%     514   $0.0078   $0.0172    6.8s
           no-short-paths      5/5     3    8.9k    59%     780   $0.0099   $0.0160    8.2s
           classic             5/5     5    9.8k    66%     372   $0.0074   $0.0154    7.6s
slug       presage             5/5     2    4.8k    68%    1.3k   $0.0109   $0.0131     12s
           no-attach           5/5     3    7.1k    77%     584   $0.0066   $0.0117     12s
           no-short-paths      5/5     2    4.9k    67%    1.1k   $0.0079   $0.0112     15s
           classic             4/5     5    7.7k    71%     390   $0.0063   $0.0125     14s   1 declared done, then failed verification
tariff     presage             5/5     2    8.4k    61%     504   $0.0082   $0.0140    8.0s
           no-attach           5/5     4   16.3k    77%     498   $0.0144   $0.0250     11s
           no-short-paths      5/5     1    4.0k    31%    1.2k   $0.0101   $0.0115     10s
           classic             5/5     6   13.6k    58%     402   $0.0118   $0.0207     13s
textstats  presage             5/5     3    7.7k    59%    1.6k   $0.0101   $0.0149     16s
           no-attach           5/5     5   13.8k    48%     553   $0.0112   $0.0215    9.7s
           no-short-paths      5/5     3    8.2k    56%     637   $0.0091   $0.0143    9.0s
           classic             5/5     7   11.4k    68%     513   $0.0084   $0.0179     14s
roman      presage             5/5     2    5.6k    48%   17.6k   $0.0847   $0.0877   2m09s
           no-attach           5/5     7   21.7k    46%   21.7k   $0.1103   $0.1413   2m34s
           no-short-paths      5/5     6   20.3k    63%   11.4k   $0.0672   $0.0813   1m53s
           classic             5/5     9   29.3k    63%   11.2k   $0.0588   $0.0676   1m40s
report     presage             5/5     2    4.3k    73%     423   $0.0045   $0.0079    4.6s
           no-attach           5/5     3    6.0k    72%     435   $0.0054   $0.0103    6.9s
           no-short-paths      5/5     2    4.3k    57%     561   $0.0055   $0.0083     12s
           classic             5/5     6    8.5k    60%     545   $0.0076   $0.0145     14s
ALL        presage           35/35     2    4.8k    65%     955   $0.0084   $0.0131     11s   total $0.699, $0.0200 per solved task
           no-attach         35/35     3    9.4k    64%     514   $0.0089   $0.0172    9.7s   total $0.859, $0.0245 per solved task
           no-short-paths    35/35     2    6.1k    58%     827   $0.0086   $0.0127     11s   total $0.735, $0.0210 per solved task
           classic           34/35     6    9.9k    66%     477   $0.0084   $0.0154     14s   total $0.791, $0.0233 per solved task   1 declared done, then failed verification
presage against classic: turns -67%, input tokens -51%, output tokens +100%, cost -0% (-15% with no cache), time -22%; solved 35/35 against 34/35
  by task (7): fewer turns on 7, more on 0; cheaper on 4, dearer on 3 (with no cache: 5 and 2)
no-attach against classic: turns -50%, input tokens -5%, output tokens +8%, cost +5% (+12% with no cache), time -31%; solved 35/35 against 34/35
  by task (7): fewer turns on 7, more on 0; cheaper on 2, dearer on 5 (with no cache: 3 and 4)
no-short-paths against classic: turns -67%, input tokens -38%, output tokens +73%, cost +2% (-17% with no cache), time -16%; solved 35/35 against 34/35
  by task (7): fewer turns on 7, more on 0; cheaper on 3, dearer on 4 (with no cache: 5 and 2)

1 run without a solution:
  glm-5.3 · slug · classic #1 · session 20261111-000501-8b97 · done; verification: FAILED (failures=2)

```
