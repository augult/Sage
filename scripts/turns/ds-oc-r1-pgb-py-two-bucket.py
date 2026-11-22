THOUGHT = r'''All known answers match, the search is minimal for both starting buckets, the
other-bucket amount is consistent, and impossible goals raise `ValueError` with a
message.'''

CONTENT = r'''Implemented `measure(bucket_one, bucket_two, goal, start_bucket)` as a breadth-first search over `(one, two)` states, starting from a single fill of the required bucket. Each state expands to the six legal actions (fill and empty each bucket, pour each way until one is empty or the other full), and any successor that leaves the starting bucket empty while the other is full is discarded. The first state holding the goal returns the action count, the bucket that holds it, and what is left in the other bucket.

Unreachable goals raise `ValueError` with an explanatory message.

Verified against the known puzzle answers — `(3, 5, 1)` from each bucket gives `(4, "one", 5)` and `(8, "two", 3)`, `(7, 11, 2)` gives `(14, "one", 11)` and `(18, "two", 7)`, and `(1, 3, 3)` starting with bucket two gives `(1, "two", 0)` — plus impossible goals such as `(2, 4, 1)` and `(6, 9, 2)` (all checks passed).'''

CALLS = []
