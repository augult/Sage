THOUGHT = "Implementation complete and verified."
CONTENT = """Implemented `solve` in `src/lib.rs` as a breadth-first search over bucket states.

Approach:
- The search starts from the single legal first action, filling the requested start bucket.
- From each state the three allowed actions are generated (fill either bucket, empty either
  bucket, pour either direction until the source is empty or the target is full).
- Every resulting state is filtered by the third rule: the start bucket may not be empty
  while the other bucket is full.
- Because the search is breadth-first over a visited set, the first state whose bucket
  contains the goal gives the fewest moves, the bucket holding it, and what remains in the
  other bucket.
- A goal larger than both capacities returns `None` before searching, and unreachable goals
  (for example 3 liters from 2 and 4) fall out of the search as `None`.

`cargo test` passes: 7 unit tests, including the canonical 1 liter in 3 and 5 (4 moves from
bucket one, 8 from bucket two), 2 liters in 7 and 11 (14 moves), both-buckets-filled cases,
equal buckets, and the impossible goals."""
CALLS = []
