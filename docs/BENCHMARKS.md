# Benchmarks

The results the website shows, from 22 November 2026: Sage against OpenCode 1.19.5 on the same 30
tasks with two models. Every run is in `docs/results/`.

**The tasks.** The 7 tasks of `sage bench`'s `hard` set (multi-file projects in Python and shell)
and 23 Exercism exercises from the polyglot set (12 Python, 11 Rust) whose whole test suite is held
out until the agent finishes. Each harness ran every task twice: 60 runs per harness and model.

**How a run is judged.** Every run starts from a fresh copy of the task. When the agent is done,
the files it was told to leave alone are put back, the held-out tests are added and the task's
verification is run. Only that counts as solved, whatever the agent claimed. OpenCode reached the
model through a local proxy that counted its requests and tokens.

To run the built-in tasks yourself (`sage bench --help` lists every option):

```sh
cargo build --release
target/release/sage bench --check --tasks all      # each task fails as shipped, passes when solved
scripts/external/install.sh opencode               # only to compare with OpenCode
target/release/sage bench -p logfare -m deepseek-v4.1-flash-1017 --tasks hard \
  --variants sage,ext:opencode -n 2 --out results.json
```

## GPT-6 Luna: 7 Hard tasks + 23 Polyglot blind tasks, 22 November 2026

120 runs (`gpt-6-luna`, 2 repeats per task and harness) across the 7 built-in `hard` tasks (28 runs) and the 23 Python and Rust Exercism `polyblind` tasks (92 runs), after adding one-turn beyond-watch spec verification, zero-test runner rejection, passive inspection filtering, and masked multi-line script failure detection (`docs/results/2026-11-22-gpt6-luna/`):

| Harness | Solved (all 60) | Hard (14) | Polyglot Rust (22) | Polyglot Python (24) | Median turns | Total turns | Median / Mean time | Prompt tokens | Cost (GPT-6 Luna list price) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| **Sage** | **30 / 60 (50.0%)** | 11 / 14 | **16 / 22 (72.7%)** | 3 / 24 | **2.0** | **171** | **41.2 s / 59.7 s** | **940.8k** | **$0.137** |
| **OpenCode 1.19.5** | **24 / 60 (40.0%)** | 12 / 14 | **9 / 22 (40.9%)** | 3 / 24 | **6.0** | **378** | **60.8 s / 78.7 s** | **2,876.9k** | **$0.174** |

Sage solved 25% more tasks overall (and 78% more on the typed Rust polyglot exercises where 100% of the test suite is held out) while being faster on both median and mean time (**49.9 s vs. 61.4 s mean** on the 23-task polyglot set) and using **55% fewer turns**, **67% fewer prompt tokens**, and **21% lower cost**. The cost is at GPT-6 Luna's list price ($0.10 per million input tokens, $0.01 cached, $0.50 output); an earlier version of this page priced the same tokens with a cache discount of only half, which made the gap look like 43%.

## DeepSeek V4.1 Flash on the same 30 tasks, 22 November 2026

The exact same 30-task, 120-run suite (7 `hard` + 23 Python and Rust `polyblind`, 2 repeats per task and harness) run with `deepseek-v4.1-flash-1017` (`docs/results/2026-11-22-deepseek-v4.1-flash/`):

| Harness | Solved (all 60) | Hard (14) | Polyglot Rust (22) | Polyglot Python (24) | Median turns | Total turns | Median / Mean time | Prompt tokens | Cost (DeepSeek rate) | Cost (OpenAI 2:1 cache) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| **Sage** | **45 / 60 (75.0%)** | **14 / 14** | **19 / 22 (86.4%)** | **12 / 24 (50.0%)** | **2.0** | **142** | **54.7 s / 74.6 s** | **683.5k** | **$0.207** | **$0.232** |
| **OpenCode 1.19.5** | **40 / 60 (66.7%)** | **14 / 14** | **18 / 22 (81.8%)** | **8 / 24 (33.3%)** | **6.0** | **378** | **86.0 s / 96.9 s** | **3,269.1k** | **$0.274** | **$0.456** |

Both harnesses solved **14 / 14** on the 7 Hard tasks, while Sage solved **31 / 46 (67.4%) vs. 26 / 46 (56.5%)** on the 23 Polyglot blind tasks (**45 / 60 vs. 40 / 60 overall**, +12.5% more solved), using **62% fewer turns** (142 vs. 378), **79% fewer prompt tokens** (683.5k vs. 3,269.1k), **36% lower median time** (54.7 s vs. 86.0 s; **71.4 s vs. 94.7 s mean** on the polyglot set), and **24.3% lower cost at DeepSeek's 50:1 cache rate** (**49.0% lower cost at OpenAI's 2:1 cache rate**).
