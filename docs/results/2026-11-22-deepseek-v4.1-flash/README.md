# DeepSeek V4.1 Flash benchmark: 7 Hard tasks + 23 Polyglot blind tasks (22 November 2026)

120 runs total (`deepseek-v4.1-flash-1017`, 2 repeats per task and harness) comparing **Sage** (with beyond-watch spec verification, zero-test runner rejection, passive inspection filtering, and masked multi-line script failure detection) against **OpenCode 1.19.5** across the exact same 30 tasks as the GPT-6 Luna evaluation:
- **7 Hard tasks** (`shipquote`, `attempts`, `connpool`, `confdialect`, `rollup`, `jobqueue`, `ttlcache`): 28 runs.
- **23 Aider/Exercism Polyglot blind tasks** (12 Python + 11 Rust exercises with 100% held-out grader test suites): 92 runs.

## Summary across all 30 tasks (120 runs)

| Harness | Solved (60 runs) | Hard set (14 runs) | Polyglot Rust (22 runs) | Polyglot Python (24 runs) | Median turns | Total turns | Median / Mean time | Prompt tokens | Cost (DeepSeek rate) | Cost (OpenAI 2:1 rate) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| **Sage** | **45 / 60 (75.0%)** | **14 / 14** | **19 / 22 (86.4%)** | **12 / 24 (50.0%)** | **2.0** | **142** | **54.7 s / 74.6 s** | **683.5k** | **$0.207** | **$0.232** |
| **OpenCode 1.19.5** | **40 / 60 (66.7%)** | **14 / 14** | **18 / 22 (81.8%)** | **8 / 24 (33.3%)** | **6.0** | **378** | **86.0 s / 96.9 s** | **3,269.1k** | **$0.274** | **$0.456** |

## Per-repeat breakdown

| Suite & Repeat | Solved | Median turns | Total turns | Median / Mean time | Prompt tokens | Output tokens | Cost (DeepSeek rate) | Cost (OpenAI 2:1 rate) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| **7 Hard — Sage, run 1** | **7 / 7** | 2 | 15 | 118.5 s / 136.4 s | 111.3k | 55.6k | $0.0424 | $0.0462 |
| **7 Hard — Sage, run 2** | **7 / 7** | 1 | 10 | 39.4 s / 34.2 s | 64.4k | 32.4k | $0.0269 | $0.0280 |
| **7 Hard — OpenCode, run 1** | **7 / 7** | 6 | 42 | 117.8 s / 161.6 s | 419.4k | 52.2k | $0.0479 | $0.0706 |
| **7 Hard — OpenCode, run 2** | **7 / 7** | 5 | 32 | 45.0 s / 47.1 s | 288.5k | 25.9k | $0.0313 | $0.0448 |
| **23 Polyglot — Sage, run 1** | **19 / 23** | 3 | 58 | 60.0 s / 83.8 s | 247.1k | 89.2k | $0.0710 | $0.0806 |
| **23 Polyglot — Sage, run 2** | **12 / 23** | 2 | 59 | 50.8 s / 58.9 s | 260.6k | 82.3k | $0.0670 | $0.0775 |
| **23 Polyglot — OpenCode, run 1** | **13 / 23** | 6 | 151 | 93.3 s / 103.8 s | 1,273.8k | 95.9k | $0.1003 | $0.1729 |
| **23 Polyglot — OpenCode, run 2** | **13 / 23** | 6 | 153 | 75.6 s / 85.6 s | 1,287.3k | 84.4k | $0.0941 | $0.1673 |

## Files

- `runs.csv`: per-run metrics for all 120 runs.
- `results.json`: full structured results including `verify_tail` for every run.
