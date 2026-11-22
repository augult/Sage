THOUGHT = r'''All checks pass, including the README board. Done.'''

CONTENT = r'''`connect.py` implements `ConnectGame(board)` with `get_winner()` (plus a `get_winner(board)` wrapper and a `winner` alias).

Board lines are split on whitespace so the leading indentation doesn't matter; neighbours follow the hex layout of the indented rows: same-row `c±1`, up `(r-1, c)` / `(r-1, c+1)`, down `(r+1, c-1)` / `(r+1, c)`. A flood fill from the left edge decides `X` (must reach each row's last column) and from the top row decides `O` (must reach the bottom row); `""` means no winner, with ragged row widths handled.

Verified on the README board (no winner), straight and winding wins for both colours, single-row/column boards, off-grid diagonals, and string input.'''

CALLS = []
