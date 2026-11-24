# GPT-6 Luna benchmark: 7 Hard tasks + 23 Polyglot blind tasks (22 November 2026)

120 runs total (`gpt-6-luna`, `low` starting effort, 2 repeats per task and harness) comparing **Sage** (with beyond-watch spec verification, zero-test runner rejection, and masked multi-line script failure detection) against **OpenCode 1.19.5** across:
- **7 Hard tasks** (`shipquote`, `attempts`, `connpool`, `confdialect`, `rollup`, `jobqueue`, `ttlcache`): 28 runs.
- **23 Aider/Exercism Polyglot blind tasks** (12 Python + 11 Rust exercises with 100% held-out grader test suites): 92 runs.

## Summary across all 30 tasks (120 runs)

| Harness | Solved (60 runs) | Hard set (14 runs) | Polyglot Rust (22 runs) | Polyglot Python (24 runs) | Median turns | Total turns | Median / Mean time | Prompt tokens | Cost (list price) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| **Sage** | **30 / 60 (50.0%)** | 11 / 14 | **16 / 22 (72.7%)** | 3 / 24 | **2.0** | **171** | **41.2 s / 59.7 s** | **940.8k** | **$0.137** |
| **OpenCode 1.19.5** | **24 / 60 (40.0%)** | 12 / 14 | **9 / 22 (40.9%)** | 3 / 24 | **6.0** | **378** | **60.8 s / 78.7 s** | **2,876.9k** | **$0.174** |

The cost is at GPT-6 Luna's list price: $0.10 per million input tokens, $0.01 cached, $0.50 output.

## Per-repeat breakdown

| Suite & Repeat | Solved | Median turns | Total turns | Median / Mean time | Prompt tokens | Output tokens | Cost (list price) |
|---|---:|---:|---:|---:|---:|---:|---:|
| **7 Hard — Sage, run 1** | **7 / 7** | 2 | 22 | 49.8 s / 75.1 s | 196.9k | 31.6k | $0.0242 |
| **7 Hard — Sage, run 2** | **4 / 7** | 2 | 24 | 85.6 s / 108.8 s | 205.3k | 35.4k | $0.0255 |
| **7 Hard — OpenCode, run 1** | **5 / 7** | 9 | 61 | 139.7 s / 122.4 s | 625.3k | 38.0k | $0.0351 |
| **7 Hard — OpenCode, run 2** | **7 / 7** | 8 | 63 | 136.6 s / 149.2 s | 676.6k | 46.9k | $0.0403 |
| **23 Polyglot — Sage, run 1** | **9 / 23** | 3 | 62 | 37.8 s / 49.0 s | 271.0k | 59.3k | $0.0423 |
| **23 Polyglot — Sage, run 2** | **10 / 23** | 2 | 63 | 37.9 s / 50.9 s | 267.5k | 63.8k | $0.0452 |
| **23 Polyglot — OpenCode, run 1** | **6 / 23** | 5 | 129 | 49.5 s / 61.1 s | 814.5k | 46.7k | $0.0496 |
| **23 Polyglot — OpenCode, run 2** | **6 / 23** | 5 | 125 | 57.5 s / 61.6 s | 760.6k | 46.5k | $0.0491 |

## Files

- `runs.csv`: per-run metrics for all 120 runs. Its `cost_ds` and `cost_oai` columns price the same tokens at DeepSeek V4.1 Flash's rates and with a cache discount of half; neither is GPT-6 Luna's price.
- `results.json`: full structured results including `verify_tail` for every run.
