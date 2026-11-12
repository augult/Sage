---
name: tests
title: Write tests
description: Tests that pin the specified behaviour and would catch real bugs.
---
Write tests that fail when the behaviour is wrong. A test that cannot fail is worse than none.

1. Learn the contract from the documentation, docstrings, types and the code. Write down every rule it promises: inputs, outputs, errors, state changes, ordering, limits. Test what callers rely on, not how it is implemented. Where the code disagrees with its documentation, do not enshrine the bug: test the documented behaviour and report the mismatch.
2. Use the project's existing framework, layout, naming and fixtures, and only what is installed.
3. For every rule cover:
   - a typical case, asserting the exact expected value (not merely "no error", a type or a length)
   - both sides of every boundary: empty, one, many; zero and negative; first and last; exactly at a limit, one below, one above
   - every branch and every documented error: the right exception, and state unchanged after a failed operation
   - special values that apply: missing or None, duplicates, unsorted input, whitespace, case and unicode, ties and equal elements, rounding at .5, very large input
   - state and sequences: operations in another order, the same call twice, two instances that must not share state, arguments that must not be mutated, the effect of one call on the next
   - options that interact with each other
4. Qualities of a good test: one behaviour per test, named after it; arrange, act, assert; precise assertions on the whole result (equality rather than `in` or truthiness); deterministic (no real clock, network or randomness: inject them or fix the seed; temporary directories for files); independent of other tests and of order; fast. Many similar cases go in a table, each case identifiable when it fails.
   Avoid: recomputing the expected value with the code's own formula, mocking the thing under test, snapshots of whatever the code currently prints, reaching into private internals.
5. Prove the tests have teeth. Break the code on purpose, one small change at a time: flip a comparison (`<` to `<=`), move a boundary or constant by one, delete a branch, swap an operator, drop a validation, return early. Each time some test must fail. A change that survives marks a missing test: add it. Do this for every rule on your list, not only a few. Then restore the code and confirm with `git diff` (or by re-reading) that the source is exactly as it was.
6. Run the whole suite twice: green both times. Do not change the code under test unless asked. If a test exposes a real bug, keep the test and report the bug.

In the summary: what is covered, rule by rule; bugs or documentation mismatches found; what is deliberately not covered and why.
