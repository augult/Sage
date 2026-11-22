# GPT-6 Luna benchmark: 7 Hard tasks + 23 Polyglot blind tasks (22 November 2026)

120 runs total (`gpt-6-luna`, `low` starting effort, 2 repeats per task and harness) comparing **Sage** (with beyond-watch spec verification, zero-test runner rejection, and masked multi-line script failure detection) against **OpenCode 1.19.5** across:
- **7 Hard tasks** (`shipquote`, `attempts`, `connpool`, `confdialect`, `rollup`, `jobqueue`, `ttlcache`): 28 runs.
- **23 Aider/Exercism Polyglot blind tasks** (12 Python + 11 Rust exercises with 100% held-out grader test suites): 92 runs.

## Summary across all 30 tasks (120 runs)

| Harness | Solved (60 runs) | Hard set (14 runs) | Polyglot Rust (22 runs) | Polyglot Python (24 runs) | Median turns | Total turns | Median / Mean time | Prompt tokens | Cost (OpenAI 2:1 cache rate) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| **Sage** | **30 / 60 (50.0%)** | 11 / 14 | **16 / 22 (72.7%)** | 3 / 24 | **2.0** | **171** | **41.2 s / 59.7 s** | **940.7k** | **$0.212** |
| **OpenCode 1.19.5** | **24 / 60 (40.0%)** | 12 / 14 | **9 / 22 (40.9%)** | 3 / 24 | **6.0** | **378** | **60.8 s / 78.7 s** | **2,876.9k** | **$0.370** |

## Per-repeat breakdown

| Suite & Repeat | Solved | Median turns | Total turns | Median / Mean time | Prompt tokens | Output tokens | Cost (DeepSeek rate) | Cost (OpenAI 2:1 rate) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| **7 Hard — Sage, run 1** | **7 / 7** | 2 | 22 | 49.8 s / 75.2 s | 196.9k | 31.6k | $0.0300 | $0.0410 |
| **7 Hard — Sage, run 2** | **4 / 7** | 2 | 24 | 85.6 s / 108.8 s | 205.3k | 35.4k | $0.0313 | $0.0444 |
| **7 Hard — OpenCode, run 1** | **5 / 7** | 9 | 61 | 139.7 s / 122.4 s | 625.3k | 38.0k | $0.0408 | $0.0700 |
| **7 Hard — OpenCode, run 2** | **7 / 7** | 8 | 63 | 136.6 s / 149.2 s | 676.6k | 46.9k | $0.0466 | $0.0808 |
| **23 Polyglot — Sage, run 1** | **9 / 23** | 3 | 62 | 37.8 s / 48.9 s | 271.0k | 59.3k | $0.0526 | $0.0642 |
| **23 Polyglot — Sage, run 2** | **10 / 23** | 2 | 63 | 37.9 s / 50.9 s | 267.5k | 63.8k | $0.0564 | $0.0672 |
| **23 Polyglot — OpenCode, run 1** | **6 / 23** | 5 | 129 | 49.5 s / 61.1 s | 814.5k | 46.7k | $0.0601 | $0.1042 |
| **23 Polyglot — OpenCode, run 2** | **6 / 23** | 5 | 125 | 57.5 s / 61.7 s | 760.6k | 46.5k | $0.0600 | $0.1001 |

## Files

- `runs.csv`: per-run metrics for all 120 runs.
- `results.json`: full structured results including `verify_tail` for every run.
