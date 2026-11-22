THOUGHT = r'''All scoring cases and error messages verified; the trailing traceback was my intentional invalid-roll probe. Done.'''

CONTENT = r'''`bowling.py` now implements `BowlingGame` (aliased as `Bowling`) with `roll`/`score`.

Scoring: per-frame roll lists; strikes and spares pull bonus pins from subsequent rolls, the tenth frame keeps its fill balls and scores as a plain sum. Validation raises `ValueError` with messages: "pin count must be between 0 and 10", "a frame cannot score more than 10 points", "cannot roll after game is over", and "score cannot be taken until the end of the game".

Verified against all-zeros, all-ones, all-fives (150), twelve strikes (300), the README three-frame example (48), mixed strike/spare games, and tenth-frame strike/spare/open fill cases, plus the four error paths.'''

CALLS = []
