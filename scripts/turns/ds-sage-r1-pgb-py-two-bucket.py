THOUGHT = r'''Breadth-first search over bucket states gives the minimum number of actions.
Each edge is one of the six actions, filtered by the rule that forbids the
state where the starting bucket is empty and the other is full. The first
state holding the goal at a given depth yields the answer, and the generation
order (fill/empty bucket one before bucket two) matches the expected tie-break.
'''

CONTENT = r'''Implementing `measure` in `two_bucket.py` with a breadth-first search over bucket states.'''

CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "two_bucket.py",
            "overwrite": True,
            "content": r"""from collections import deque


def measure(bucket_one, bucket_two, goal, start_bucket):
    if start_bucket not in ("one", "two"):
        raise ValueError("start_bucket must be 'one' or 'two'")
    queue = deque([(0, 0, 0)])
    seen = {(0, 0)}
    while queue:
        one, two, moves = queue.popleft()
        for next_one, next_two in _actions(one, two, bucket_one, bucket_two):
            if (next_one, next_two) in seen:
                continue
            if start_bucket == "one":
                if next_one == 0 and next_two == bucket_two:
                    continue
            elif next_one == bucket_one and next_two == 0:
                continue
            seen.add((next_one, next_two))
            next_moves = moves + 1
            if next_one == goal:
                return next_moves, "one", next_two
            if next_two == goal:
                return next_moves, "two", next_one
            queue.append((next_one, next_two, next_moves))
    raise ValueError("no solution for these bucket sizes and goal")


def _actions(one, two, cap_one, cap_two):
    yield cap_one, two
    yield one, cap_two
    yield 0, two
    yield one, 0
    poured = min(one, cap_two - two)
    yield one - poured, two + poured
    poured = min(two, cap_one - one)
    yield one + poured, two - poured
""",
        },
    },
    {
        "name": "shell",
        "arguments": {
            "expect_exit": "0",
            "cmd": r"""python3 - <<'PY'
from two_bucket import measure

cases = [
    ((3, 5, 1, "one"), (4, "one", 5)),
    ((3, 5, 1, "two"), (8, "two", 3)),
    ((7, 11, 2, "one"), (14, "one", 11)),
    ((7, 11, 2, "two"), (18, "two", 7)),
    ((1, 3, 3, "two"), (1, "two", 0)),
    ((2, 3, 3, "one"), (2, "two", 2)),
    ((1, 1, 1, "one"), (1, "one", 0)),
]
for args, expected in cases:
    got = measure(*args)
    assert got == expected, (args, got, expected)

try:
    measure(2, 4, 3, "one")
except ValueError as error:
    assert str(error)
else:
    raise AssertionError("expected ValueError for an unreachable goal")

try:
    measure(3, 5, 1, "three")
except ValueError as error:
    assert str(error)
else:
    raise AssertionError("expected ValueError for a bad start bucket")
print("all checks passed")
PY""",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented `measure` in `two_bucket.py` with BFS over bucket states, the six allowed actions and the rule forbidding an empty starting bucket with a full other bucket; raises `ValueError` for an unreachable goal or bad start bucket. Verified against the exercise's expected move counts including both starting buckets, plus unreachable and invalid-input cases.",
            "unverified": "",
        },
    },
]
