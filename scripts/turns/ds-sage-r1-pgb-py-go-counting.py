THOUGHT = r'''My inline check had two wrong expectations, not a code bug: with board ["B B"] the
single empty intersection is bordered only by black stones, so it is black territory, and with
["B W", "   "] the empty column between the stones joins the empty row below, giving a
four-cell region bordered by both colours. I will correct the expectations and finish.'''

CONTENT = r'''That assertion failure was in my check, not the code: `"B B"` leaves its one empty intersection bordered only by black, so it is black territory. Correcting the expectations and rerunning.'''

CALLS = [
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''python3 -c '
from go_counting import BLACK, NONE, WHITE, Board

b5 = Board(["  B  ", " B B ", "B W B", " W W ", "  W  "])
assert b5.shape == (5, 5)
assert b5.territory(0, 1) == (BLACK, {(0, 0), (1, 0), (0, 1)}), b5.territory(0, 1)
assert b5.territory(2, 2) == (WHITE, set()), "stone coordinate"
assert b5.territory(2, 1) == (NONE, {(2, 1)}), b5.territory(2, 1)
t5 = b5.territories()
assert t5[BLACK] == {(0, 0), (1, 0), (0, 1), (3, 0), (4, 0), (4, 1)}, t5[BLACK]
assert t5[WHITE] == {(2, 3)}, t5[WHITE]
assert t5[NONE] == {(2, 1), (1, 2), (3, 2), (0, 3), (0, 4), (1, 4), (3, 4), (4, 4), (4, 3)}, t5[NONE]
assert set(t5) == {BLACK, WHITE, NONE}

single_stone = Board(["B"])
assert single_stone.territory(0, 0) == (BLACK, set())
assert single_stone.territories() == {BLACK: set(), WHITE: set(), NONE: set()}

open_board = Board(["   ", "   "])
assert open_board.territory(1, 1) == (NONE, {(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)})
assert open_board.territories() == {BLACK: set(), WHITE: set(), NONE: {(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)}}

enclosed = Board(["B B"])
assert enclosed.territory(1, 0) == (BLACK, {(1, 0)}), enclosed.territory(1, 0)
assert enclosed.territories()[BLACK] == {(1, 0)}

mixed = Board(["B W", "   "])
assert mixed.territory(1, 0) == (NONE, {(1, 0), (0, 1), (1, 1), (2, 1)}), mixed.territory(1, 0)
assert mixed.territories()[NONE] == {(1, 0), (0, 1), (1, 1), (2, 1)}

for x, y in ((-1, 0), (5, 0), (0, -1), (0, 5)):
    try:
        b5.territory(x, y)
    except ValueError as exc:
        assert str(exc), "ValueError needs a message"
    else:
        raise AssertionError("no ValueError for %s,%s" % (x, y))

print("edge cases ok")
' ''',
            "expect_exit": "0",
            "expect_has": "edge cases ok",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented `go_counting.py` with `WHITE`/`BLACK`/`NONE` constants and a flood-fill `Board`. `territory(x, y)` raises `ValueError(\"Invalid coordinate\")` off-board, returns the stone colour with an empty set on an occupied intersection, and otherwise returns the region's single bordering colour (or `\"\"` when both colours border it) along with every coordinate in the region. `territories()` groups all empty regions board-wide under `\"B\"`, `\"W\"` and `\"\"`.",
            "unverified": "The grader's own tests are hidden; behaviour was verified against the canonical 5x5 board, single-stone and open boards, a black-enclosed intersection, a mixed-colour border, whole-board grouping and out-of-range coordinates.",
        },
    },
]
