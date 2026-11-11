---
name: debug
title: Debug
description: Reproduce the bug, find the root cause, fix it there, and pin it with a test.
---
Fix the cause, not the symptom.

1. Reproduce first. Make the failure happen on demand with the smallest command or test, and note the exact error. If it will not reproduce, gather more (logs, inputs, versions, order of operations) instead of fixing blind. If it is intermittent, run it in a loop and vary order, seed and timing until you find the trigger.
2. Read the failure completely: the whole message and trace, the first error rather than the last, the frame in project code, actual against expected values.
3. Locate by evidence. Trace the wrong value backwards from where it is observed to the first place it is wrong: print or assert intermediate values, halve the input or the pipeline, and with git history ask what changed. State a hypothesis that explains every observation, including why the other cases work, and test it with one experiment before editing. If the experiment contradicts it, drop the hypothesis. Never stack a second fix on a first one that did not work: undo the first.
4. When the symptom is far from the cause, suspect:
   - shared mutable state: module globals, default arguments, class attributes, caches, singletons
   - a cache key that leaves out one of the inputs
   - state leaking between tests, so that order matters
   - an alias where a copy was meant; an iterator consumed twice
   - boundaries: off-by-one, rounding, integer against float, timezone and daylight saving, encoding
   - assumptions about ordering (dicts, sets, unstable sorts)
   - threads and async: unsynchronised read-modify-write, check-then-act
   - an `except` that swallows the real error; stale build output; a different environment
5. Fix at the root, as small as possible. Then ask where else the same mistake lives: the same pattern nearby, sibling functions, other callers of the broken helper. Fix those or report them. Do not mask the bug: no broad `try`/`except`, no special case for the failing input, no retry or sleep over a race, no editing the test to agree with the code.
6. Pin it. Add a regression test that fails without your fix and passes with it, and check both for real: run it against the original code (stash or temporarily undo the fix) and see it fail. Name it after the behaviour. Put it where the task says, otherwise next to the related tests.
7. Verify widely. The reproduction passes, the whole suite passes, and where state or timing was involved it passes repeatedly and in a different order. Remove all debug output.

In the summary: the root cause in one or two sentences (what was wrong, and why it showed up where it did), the fix, the regression test, and anything related you saw but did not change.
