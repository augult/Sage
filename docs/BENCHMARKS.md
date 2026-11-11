# Benchmarks

`presage bench` runs 7 small synthetic tasks (Python and shell: fix bugs, implement a function from
a README, write a module that has no tests) under several harness variants. Every run starts from a
fresh copy of the task. When the agent is done, the files it was told to leave alone are put back,
held-out checks are added and the task's verification is run. Only that counts as solved.
`presage bench --check` proves that each task fails as shipped and passes with its reference
solution.

All runs below: Logfare gateway, effort `ratchet`, 5 runs per task and variant (35 per variant).
Figures are medians per run unless stated. "No cache" is the list-price cost had the provider
cached nothing, which takes the luck of the cache out of a comparison. Raw results and the CLI's own
tables are in `docs/results/`.

```sh
cargo build
target/debug/presage bench --check
LOGFARE_API_KEY=... PRESAGE_HOME=/tmp/presage-live target/debug/presage bench -p logfare -m glm-5.3 \
  --variants presage,no-preflight,no-auto-watch,classic -n 5 -j 3 --out /tmp/glm-5.3.json
```

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
