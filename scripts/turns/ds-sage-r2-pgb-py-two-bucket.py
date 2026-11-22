THOUGHT = r'''`measure` is a stub. Shortest route is a breadth-first search over
`(bucket_one, bucket_two)` states with the three allowed actions. The first
fill is forced to the named start bucket, and a state where the start bucket is
empty while the other is full is never entered. BFS depth gives the action
count; the goal check reports which bucket holds the goal and what the other
holds. Goals above the larger bucket, or unreachable by the gcd, raise
`ValueError`.'''

CONTENT = r'''Implementing `measure` with a breadth-first search over bucket states (fill,
empty, pour), respecting the start-bucket and no-forbidden-state rules, then
checking it against the canonical measurements and error cases.'''

CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "two_bucket.py",
            "overwrite": True,
            "content": r'''from collections import deque
from math import gcd


def measure(bucket_one, bucket_two, goal, start_bucket):
    if goal > max(bucket_one, bucket_two):
        raise ValueError("Goal is larger than both buckets")
    if goal % gcd(bucket_one, bucket_two):
        raise ValueError("No solution possible")

    caps = {"one": bucket_one, "two": bucket_two}
    start = start_bucket
    other = "two" if start == "one" else "one"

    def key(state):
        return (state["one"], state["two"])

    def forbidden(state):
        return state[start] == 0 and state[other] == caps[other]

    first = {start: caps[start], other: 0}
    queue = deque([(first, 1)])
    seen = {key(first)}

    while queue:
        state, moves = queue.popleft()
        if state["one"] == goal:
            return moves, "one", state["two"]
        if state["two"] == goal:
            return moves, "two", state["one"]

        neighbours = []
        for name in ("one", "two"):
            filled = dict(state)
            filled[name] = caps[name]
            neighbours.append(filled)
            emptied = dict(state)
            emptied[name] = 0
            neighbours.append(emptied)
        for src, dst in (("one", "two"), ("two", "one")):
            amount = min(state[src], caps[dst] - state[dst])
            if amount:
                poured = dict(state)
                poured[src] = state[src] - amount
                poured[dst] = state[dst] + amount
                neighbours.append(poured)

        for nxt in neighbours:
            nk = key(nxt)
            if nk in seen or forbidden(nxt):
                continue
            seen.add(nk)
            queue.append((nxt, moves + 1))

    raise ValueError("No solution possible")
''',
        },
    },
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''cat > /tmp/check_two_bucket.py <<'EOF'
import os
import sys
sys.path.insert(0, os.getcwd())
from two_bucket import measure

assert measure(3, 5, 1, "one") == (4, "one", 5)
assert measure(3, 5, 1, "two") == (8, "two", 3)
assert measure(7, 11, 2, "one") == (14, "one", 11)
assert measure(7, 11, 2, "two") == (18, "two", 7)
# goal already in the start bucket after the single fill
assert measure(3, 5, 3, "one") == (1, "one", 0)
assert measure(3, 5, 5, "two") == (1, "two", 0)
# equal-sized buckets
assert measure(6, 6, 6, "one") == (1, "one", 0)
for args in [(1, 3, 5, "one"), (3, 5, 8, "one"), (6, 4, 3, "one")]:
    try:
        measure(*args)
    except ValueError:
        pass
    else:
        raise AssertionError(f"{args} did not raise")
print("all checks passed")
EOF
python3 /tmp/check_two_bucket.py; rc=$?; rm -f /tmp/check_two_bucket.py; exit $rc''',
            "expect_has": "all checks passed",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented `measure` in `two_bucket.py` as a breadth-first search over bucket states: fill, empty and pour actions, with the first action forced to fill the named start bucket and any state where the start bucket is empty while the other is full excluded. Returns `(moves, goal_bucket, other_bucket_amount)`; raises `ValueError` when the goal exceeds the larger bucket or is unreachable (gcd). Verified with a Python check covering the four canonical measurements (4/8/14/18 moves), single-fill goals, equal buckets and both error cases (all checks passed).",
            "unverified": "",
        },
    },
]
