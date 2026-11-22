THOUGHT = r'''The implementation and checks passed: scoring from gutters through a perfect 300 game, tenth-frame fill balls, and every validation path (out-of-range pins, frame totals over 10, fill balls over 10, and rolls after the game ends). Nothing else is required, so I'm done.'''
CONTENT = ""
CALLS = [
    {
        "name": "finish",
        "arguments": {
            "summary": r'''Implemented BowlingGame in bowling.py with roll(pins) and score(): ten frames, open/spare/strike tabulation, tenth-frame fill balls (one for a spare, two for a strike), and ValueError with a meaningful message for pins outside 0-10, a frame over 10, fill balls over 10, and rolls after the game is complete.

Verified with an inline test script: gutters (0), all ones (20), all nines/spares, spare fills, XXX tenth frame (30), strike/spare tenth frames (20), a 300 perfect game, a 133 mixed game, plus every error path. All checks passed.'''
        },
    },
]
