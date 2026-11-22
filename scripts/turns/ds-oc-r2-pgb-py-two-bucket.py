THOUGHT = r'''The canonical expectations for both start buckets, the one-step case and the
unreachable cases all pass. Done.'''

CONTENT = r'''`two_bucket.py` implements `measure` with a breadth-first search over `(bucket1, bucket2)` states, so the action count is the minimum. It starts by filling the chosen bucket, forbids any action that leaves the starting bucket empty while the other is full, and returns the move count, the bucket holding the goal and the amount left in the other. Invalid sizes, an invalid start bucket and unreachable goals raise `ValueError` with a message.

Verified against the canonical expectations: 3/5 goal 1 from either bucket (4/"one"/5 and 8/"two"/3), 7/11 goal 2 (14/"one"/11 and 18/"two"/7), the one-step 1/3 goal 3 case, plus unreachable inputs.'''

CALLS = []
