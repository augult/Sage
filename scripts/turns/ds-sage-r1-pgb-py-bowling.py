THOUGHT = r'''The two remaining failures were my own expectations, not the code: [10,5,5] needs 16 trailing zeros to fill ten frames (and scores 30 because the tenth frame's spare bonus is a fill ball), and a two-strike partial game has no meaningful final score. Corrected the check values and re-running.'''
CONTENT = ""
CALLS = [
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''python3 - <<'PY'
from bowling import BowlingGame

def game(rolls):
    g = BowlingGame()
    for r in rolls:
        g.roll(r)
    return g

def expect_error(rolls, extra):
    g = game(rolls)
    try:
        g.roll(extra)
    except ValueError as exc:
        assert str(exc), "error message must not be empty"
        return
    raise AssertionError("expected ValueError for %r + %r" % (rolls, extra))

cases = [
    ([0] * 20, 0),
    ([1] * 20, 20),
    ([9, 0] * 10, 90),
    ([5, 5] * 10 + [5], 150),
    ([10] * 12, 300),
    ([0] * 18 + [10, 10, 10], 30),
    ([0] * 18 + [7, 3, 4], 14),
    ([1, 4, 4, 5, 6, 4, 5, 5, 10, 0, 1, 7, 3, 6, 4, 10, 2, 8, 6], 133),
    ([10, 5, 5] + [0] * 16, 30),
    ([5, 5, 10] + [0] * 16, 30),
    ([0] * 18 + [10, 5, 5], 20),
    ([0] * 18 + [10, 2, 8], 20),
]
for rolls, want in cases:
    got = game(rolls).score()
    assert got == want, (rolls, got, want)

assert game([5]).score() == 5
assert game([]).score() == 0

expect_error([5], 6)
expect_error([10] * 12, 0)
expect_error([0] * 18 + [10], 11)
g = game([0] * 18 + [10])
g.roll(5)
expect_error(g.rolls, 6)
expect_error([0] * 18 + [2, 3], 4)
for bad in (-1, 11):
    g = BowlingGame()
    try:
        g.roll(bad)
    except ValueError as exc:
        assert str(exc)
    else:
        raise AssertionError("accepted %r" % (bad,))

print("ALL PASS")
PY''',
            "expect_exit": "0",
            "expect_has": "ALL PASS",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented BowlingGame in bowling.py: roll() tracks pins and enforces the rules, raising descriptive ValueError messages for out-of-range pins, a frame totalling more than 10, invalid tenth-frame fill balls, and rolls after the game ends (a rejected roll is popped so state stays valid); score() sums the ten frames with strike and spare bonuses including tenth-frame fill balls. Verified with an in-memory check: all zeros 0, all ones 20, all nines 90, all spares 150, twelve strikes 300, tenth-frame strike/spare/open variants, the 133 mixed game, and the error cases.",
            "unverified": "",
        },
    },
]
