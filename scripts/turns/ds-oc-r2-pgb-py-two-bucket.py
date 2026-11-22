THOUGHT = r'''A breadth-first search over bucket states gives the fewest actions. The first move must
fill the starting bucket, and any action that leaves the starting bucket empty while the other
is full is forbidden per rule 3.'''

CONTENT = r'''Implementing a BFS over (bucket1, bucket2) states.'''

CALLS = [
    {"name": "write", "arguments": {"filePath": "two_bucket.py", "content": r'''from collections import deque


def measure(bucket_one, bucket_two, goal, start_bucket):
    if bucket_one <= 0 or bucket_two <= 0:
        raise ValueError("bucket sizes must be positive")
    if start_bucket not in ("one", "two"):
        raise ValueError("start bucket must be 'one' or 'two'")
    if goal > max(bucket_one, bucket_two) or goal <= 0:
        raise ValueError("goal cannot be reached with these buckets")

    sizes = (bucket_one, bucket_two)

    def forbidden(state):
        # Rule 3: the starting bucket may not be empty while the other is full.
        if start_bucket == "one":
            return state[0] == 0 and state[1] == bucket_two
        return state[1] == 0 and state[0] == bucket_one

    def next_states(state):
        a, b = state
        results = [
            (bucket_one, b),  # fill one
            (a, bucket_two),  # fill two
            (0, b),           # empty one
            (a, 0),           # empty two
        ]
        # pour one -> two
        amount = min(a, bucket_two - b)
        results.append((a - amount, b + amount))
        # pour two -> one
        amount = min(b, bucket_one - a)
        results.append((a + amount, b - amount))
        return results

    first = (bucket_one, 0) if start_bucket == "one" else (0, bucket_two)
    if forbidden(first) or first in ((0, 0),):
        raise ValueError("goal cannot be reached with these buckets")

    queue = deque([(first, 1)])
    seen = {first}
    while queue:
        state, moves = queue.popleft()
        if goal in state:
            bucket = "one" if state[0] == goal else "two"
            other = state[1] if bucket == "one" else state[0]
            return moves, bucket, other
        for nxt in next_states(state):
            if nxt in seen or forbidden(nxt):
                continue
            seen.add(nxt)
            queue.append((nxt, moves + 1))
    raise ValueError("goal cannot be reached with these buckets")
'''}},
    {"name": "bash", "arguments": {"command": r'''python3 - <<'EOF'
from two_bucket import measure

assert measure(3, 5, 1, "one") == (4, "one", 5), measure(3, 5, 1, "one")
assert measure(3, 5, 1, "two") == (8, "two", 3), measure(3, 5, 1, "two")
assert measure(7, 11, 2, "one") == (14, "one", 11), measure(7, 11, 2, "one")
assert measure(7, 11, 2, "two") == (18, "two", 7), measure(7, 11, 2, "two")
assert measure(1, 3, 3, "two") == (1, "two", 0), measure(1, 3, 3, "two")
for args in [(6, 4, 1, "one"), (0, 4, 1, "one"), (4, 4, 5, "one")]:
    try:
        measure(*args)
    except ValueError as e:
        assert str(e)
    else:
        raise AssertionError(("expected ValueError", args))
print("all checks passed")
EOF'''}},
]
