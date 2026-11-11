---
name: performance
title: Performance
description: Measure, find the real bottleneck, and make it faster without changing results.
---
Make it measurably faster with identical behaviour. Never optimise on a guess.

1. Fix the target: which operation, at what input size, and what goal (a stated limit, or as fast as is reasonable). Build a repeatable benchmark on realistic and large input. Generate the input if needed: the problem often shows only at scale. Time the baseline over several runs.
2. Lock the behaviour before changing anything. Capture current results on varied inputs, including empty input, duplicates, ties and unusual order: the existing tests plus a quick old-against-new comparison script. Results must stay identical, including ordering, tie-breaking, rounding and errors.
3. Profile. Do not trust intuition, and do not trust a comment that says which part is slow. Use `cProfile`, `node --cpu-prof`, `perf`, `EXPLAIN QUERY PLAN` or timers around sections. Find where the time really goes and how it grows: when the input doubles, does the time double or quadruple?
4. Remove the largest cost first, preferring fixes in this order:
   a. Algorithm and data structure. Nested loops over the same data become a dict or set index. Repeated linear work (`x in list`, `list.index`, `.count`, `pop(0)`, `insert(0, …)`) becomes a set, dict, deque or heap. Sorting inside a loop becomes sorting once, `bisect` or a heap. String concatenation in a loop becomes `join`. Repeated computation is done once or memoised with a correct key.
   b. I/O and queries. A query per row becomes one query, a join or a batched `IN`. Add the missing index. One commit per row becomes one transaction or `executemany`. A file read or parsed repeatedly is read once. Many small reads or writes are buffered.
   c. Avoiding work: exit early, stream instead of materialising, precompute, cache with invalidation.
   d. Only then micro-optimisations and concurrency, and only where measurement shows they matter.
5. After every change run the same benchmark and the equivalence check again. Keep a change only for a clear gain; revert the ones that bring none, they are only risk. Then profile again, because the bottleneck has moved. Stop when the target is met with a wide margin (machines differ: aim far below a stated limit) or the remaining cost is inherent.
6. Keep the public interface and the readability. No new dependencies unless allowed. Do not trade quadratic time for quadratic memory. A cache needs a complete key and a bound.

In the summary: baseline and result (input size, timings), what the bottleneck was, what you changed, and how you verified that results are identical.
