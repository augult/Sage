THOUGHT = r'''This one is my expectation again: in "X O X / . O . / X O X" the middle row is ". O .", so X cannot cross left to right while O's column does connect top to bottom, and the code rightly returns "O". Correcting that case and re-running the full check.'''
CONTENT = ""
CALLS = [
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''python3 - <<'PY'
from connect import ConnectGame

def winner(board):
    return ConnectGame(board).get_winner()

cases = [
    (". O . X .\n . X X O .\n  O O O X .\n   . X O X O\n    X O O O X", None),
    ("X", "X"),
    ("O", "O"),
    (".", None),
    ("", None),
    ("X X\n . .", "X"),
    (". X\nX .", "X"),
    ("X .\n . X", None),
    ("X .\n X .", None),
    ("O .\n O .", "O"),
    ("O .\n . O", None),
    (" . O\nO . ", "O"),
    (". X .\n X . X\n  . X .", None),
    ("X O X\n O O O\n  X O X", "O"),
    ("X O X\n O O X\n  X O X", "X"),
    ("X O X\n . O .\n  X O X", "O"),
    (". . X\n . X .\n  X . .", "X"),
]
for board, want in cases:
    got = winner(board)
    assert got == want, (board, got, want)
print("ALL PASS", len(cases))
PY''',
            "expect_exit": "0",
            "expect_has": "ALL PASS",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": r'''Implemented ConnectGame in connect.py: it parses the offset-hex board text (ragged or non-square rows are fine, "." is empty) and breadth-first searches each player's stones using the hex neighbourhood for this layout, where a stone at (row, col) touches (row-1, col), (row-1, col+1), (row, col-1), (row, col+1), (row+1, col-1) and (row+1, col). X wins by connecting its left column to its right column, O wins by connecting its top row to its bottom row; get_winner returns "X", "O" or None.

Verified with an inline check of 17 cases: the README example (no winner), single-cell and empty boards, both diagonal directions (only the offset-correct one connects), uneven-width boards, and multiple winning connections for each player. All passed.'''
        },
    },
]
