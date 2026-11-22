THOUGHT = "Correct the independently recomputed strike/spare reference score and complete the expanded test audit."
CONTENT = ""
CALLS = [
    {"name": "shell", "arguments": {
        "cmd": "python3 - <<'PY'\nfrom bowling import BowlingGame\ndef play(rs):\n    g = BowlingGame()\n    for r in rs: g.roll(r)\n    return g\ndef rejected(rs):\n    g = BowlingGame()\n    try:\n        for r in rs: g.roll(r)\n    except ValueError: return\n    raise AssertionError(('accepted invalid rolls', rs))\ng = BowlingGame()\ntry: g.score()\nexcept IndexError: pass\nelse: raise AssertionError('scored incomplete game')\nassert play([0] * 20).score() == 0\nassert play([1, 4] * 10).score() == 50\nassert play([5, 5, 3] + [0] * 17).score() == 16\nassert play([10, 3, 4] + [0] * 16).score() == 24\nassert play([10] * 12).score() == 300\nassert play([10] * 9 + [10, 1, 9]).score() == 281\nassert play([10] * 9 + [5, 5, 10]).score() == 275\nfor rs in ([11], [-1], [5, 6], [9, 2] + [0] * 18, [0] * 18 + [5, 6, 1], [0] * 18 + [10, 8, 3]): rejected(rs)\ng = play([0] * 20)\ntry: g.roll(0)\nexcept IndexError: pass\nelse: raise AssertionError('accepted extra roll')\nprint('full rule audit passed')\nPY",
        "expect_exit": "0",
        "expect_has": "full rule audit passed"
    }},
    {"name": "finish", "arguments": {
        "summary": "Implemented ten-frame bowling scoring with validation for legal pin totals, frames and fill balls, complete-game scoring, and strike/spare bonuses. Expanded rule audit passed for perfect/gutter games, open frames, spares, strikes, tenth-frame fills, invalid sequences, premature scoring, and extra rolls.",
        "unverified": "The grader's full test suite was not available."
    }}
]
