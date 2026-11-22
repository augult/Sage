# GPT-6 Luna benchmark: 7 Hard tasks + 23 Polyglot blind tasks (22 November 2026)

120 runs total (`gpt-6-luna`, `low` starting effort, 2 repeats per task and harness) comparing **Sage** (with beyond-watch spec verification, zero-test runner rejection, and masked multi-line script failure detection) against **OpenCode 1.19.5** across:
- **7 Hard tasks** (`shipquote`, `attempts`, `connpool`, `confdialect`, `rollup`, `jobqueue`, `ttlcache`): 28 runs.
- **23 Aider/Exercism Polyglot blind tasks** (12 Python + 11 Rust exercises with 100% held-out grader test suites): 92 runs.

## Summary across all 30 tasks (120 runs)

| Harness | Solved (60 runs) | Hard set (14 runs) | Polyglot Rust (22 runs) | Polyglot Python (24 runs) | Median turns | Total turns | Prompt tokens | Cost (OpenAI 2:1 cache rate) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| **Sage** | **29 / 60 (48.3%)** | 11 / 14 | **15 / 22 (68.2%)** | 3 / 24 | **2.0** | **176** | **963.9k** | **$0.221** |
| **OpenCode 1.19.5** | **23 / 60 (38.3%)** | 12 / 14 | **9 / 22 (40.9%)** | 2 / 24 | **5.5** | **375** | **2,839.6k** | **$0.366** |

## Per-repeat breakdown

| Suite & Repeat | Solved | Median turns | Total turns | Prompt tokens | Output tokens | Cost (DeepSeek rate) | Cost (OpenAI 2:1 rate) |
|---|---:|---:|---:|---:|---:|---:|---:|
| **7 Hard — Sage, run 1** | **7 / 7** | 2 | 22 | 196.9k | 31.6k | $0.0300 | $0.0410 |
| **7 Hard — Sage, run 2** | **4 / 7** | 2 | 24 | 205.3k | 35.4k | $0.0313 | $0.0444 |
| **7 Hard — OpenCode, run 1** | **5 / 7** | 9 | 61 | 625.3k | 38.0k | $0.0408 | $0.0700 |
| **7 Hard — OpenCode, run 2** | **7 / 7** | 8 | 63 | 676.6k | 46.9k | $0.0466 | $0.0808 |
| **23 Polyglot — Sage, run 1** | **9 / 23** | 3 | 66 | 289.4k | 71.9k | $0.0596 | $0.0709 |
| **23 Polyglot — Sage, run 2** | **9 / 23** | 2 | 64 | 272.3k | 63.3k | $0.0561 | $0.0643 |
| **23 Polyglot — OpenCode, run 1** | **5 / 23** | 5 | 126 | 777.1k | 46.3k | $0.0594 | $0.1089 |
| **23 Polyglot — OpenCode, run 2** | **6 / 23** | 5 | 125 | 760.6k | 46.5k | $0.0600 | $0.1066 |

## Files

- `runs.csv`: per-run metrics for all 120 runs.
- `results.json`: full structured results including `verify_tail` for every run.
