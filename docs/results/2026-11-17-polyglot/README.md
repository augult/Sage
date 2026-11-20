# Python, Rust and Go benchmark, 16Ã¢â‚¬â€œ17 November 2026

36 tasks from the Exercism polyglot set (12 Python, 12 Rust, 12 Go), each run twice per harness:
72 runs per harness and model. The agent gets the task text and its starting files; the tests are
held out and run only after it finishes. A run counts as solved only if every hidden test passes.
The original benchmark used a local build of `sage bench`. The public results include the task IDs
and grading output. The original wrapper and exact prompt text are unavailable.

## Rerun the 36 exercises

From a Sage source checkout on Linux, with Git, Python 3, Go and the pinned Rust toolchain on PATH:

```sh
python3 docs/results/2026-11-17-polyglot/prepare.py /tmp/sage-polyblind
cd /tmp/sage-polyblind
cargo build --locked --release -p sage-cli
target/release/sage bench --check --tasks polyblind
```

The script prepares a separate checkout. It selects the exercise IDs from `runs.csv` and fetches
their starters, tests, and reference implementations from [Aider's pinned Exercism snapshot](https://github.com/Aider-AI/polyglot-benchmark/tree/7e0611e77b54e2dea774cdc0aa00cf9f7ed6144f).
The offline check grades both the untouched starter and the reference solution for each exercise.
The held-out files and solutions are embedded only in this benchmark build.

For a new model run, use the benchmark executable from an isolated agent account as described below.
For example, with a configured Logfare key:

```sh
target/release/sage bench --tasks polyblind --variants sage -n 2 -p logfare -m glm-5.3 \
  -e ratchet --max-turns 60 --time-budget 1200 --out glm-rerun.json
```

This is a reconstruction for new measurements. It uses the upstream exercise instructions and a
new common prompt. For the 30 exercises that have line counts in the archived audits, the held-out
test and reference solution line counts match. The archived measurements below refer to the original runs.

## Results

**DeepSeek V4.1 Flash** (Logfare), 16Ã¢â‚¬â€œ17 November, Sage as of commit `31eb400`:

| Harness | Solved | Python | Rust | Go | Total cost | Time per run |
|---|---|---|---|---|---|---|
| Ecosage | 44/72 | 13/24 | 17/24 | 14/24 | $2.73 | 3.8 min |
| OpenCode 1.19.5 | 44/72 | 13/24 | 17/24 | 14/24 | $4.51 | 6.2 min |
| Prosage (since removed) | 41/72 | 11/24 | 17/24 | 13/24 | $7.12 | 7.1 min |
| Unharnessed | 39/72 | 13/24 | 17/24 | 9/24 | $5.13 | 8.7 min |

**GLM-5.3** (Logfare), 17 November, Sage 0.3.0:

| Harness | Solved | Python | Rust | Go | Total cost | Time per run |
|---|---|---|---|---|---|---|
| Ecosage | 31/72 | 8/24 | 15/24 | 8/24 | $8.86 | 4.6 min |
| OpenCode 1.19.5 | 19/72 | 3/24 | 9/24 | 7/24 | $21.65 | 11.5 min |

Gaps of two or three runs are within chance. Costs are what the counted runs cost; runs thrown away
(below) were paid for too and are not in them.

## How the runs were kept fair

- **Isolation.** Each harness ran under its own Linux account (two per harness, one per repeat),
  with its own home and `/tmp`, no network except the model gateway (an allow-list proxy and
  per-account firewall rules), and the benchmark binary executable but not readable, so the
  hidden tests could not be extracted from it. `scripts/setup.sh` builds this and checks it.
- **Same limits.** 20 minutes and 60 model requests per run, effort `ratchet` for Sage. External
  harnesses get no effort setting from the benchmark; OpenCode sends `reasoning_effort: "low"` to
  GLM-5.3 on its own (checked against a recording server), the same as Sage's starting level.
- **Thrown away and run again:** runs that ended on a provider error (HTTP 429/503, "temporarily
  unavailable", "did not respond in time", "at capacity"), and on DeepSeek every run that finished
  after the benchmark machine's disk filled up at 05:44 on 16 November (`raw/deepseek/excluded-disk-full.txt`).
  Sage retries provider errors inside a run; OpenCode exits, so nearly all reruns were OpenCode's.
- **Transcript audit.** Every counted transcript was checked for the hidden tests' and the
  reference solution's lines (`scripts/audit_*.py`, output in `audit/`). On DeepSeek five runs held
  90% or more of one, most likely from memory since the exercises are public: Ecosage 1 (not
  solved), OpenCode 2 (solved), Prosage 1 and Unharnessed 1 (not solved). Without them Ecosage is
  44/71 and OpenCode 42/70. On GLM-5.3 no solved run held either.

## What the logs show

- **Why Ecosage costs less (DeepSeek, counted runs with token data):** fewer requests (10.8 against
  28.5 per run) and about half the prompt per request (10.6k against 23.1k tokens), so about a
  sixth of the input; it wrote slightly more per request (2.1k against 1.7k tokens).
- **Why OpenCode was slow on GLM-5.3:** each of its requests wrote about 3,800 tokens against
  Ecosage's 680 and took 30 against 17 seconds, at the same effort setting. 15 of its runs hit the
  20-minute limit and 3 the 60-request limit; Ecosage hit a limit once.
- **OpenCode looks for the answer online.** It tried to fetch Exercism's canonical test data
  (WebFetch or `curl`) in 52 of 101 DeepSeek runs and 77 of 102 GLM runs; the isolation blocked
  every attempt, and each one cost a request.

## Files

- `runs.csv`: one row per counted run (model, harness, account, task, language, solved, cost,
  seconds, round).
- `raw/<model>/<round>/<account>/`: the benchmark's own output for each round, `results.json` with
  per-run statistics (tokens, requests, cost) and the console log. Rounds after the first rerun
  what was missing; `runs.csv` says which round each counted run is from.
- `scripts/`: the isolation and orchestration as run (`setup.sh`, `arm*.sh`, `loop*.sh`,
  `allowproxy.py`), the tallies and the audits. Paths in them are those of the machine they ran on;
  the API key was read from a file outside the repository.
- `audit/`: per-run audit lines for both models.
