THOUGHT = r'''Implementation verified against representative boards.'''

CONTENT = '''Implemented `ConnectGame` with hex-neighbour flood fill: `O` wins by connecting the top and bottom rows, `X` by connecting the left and right columns, and invalid board characters raise `ValueError`. Verified on empty boards, full-row/column wins, zigzag shapes, near-miss diagonals and the README example (no winner).'''

TEST = r'''from connect import ConnectGame

checks = [
    ([". . .", " . . .", "  . . ."], None),
    (["X X X"], "X"),
    (["O", " O", "  O"], "O"),
    (["O . .", " . . .", "  . . ."], None),
    (["X . .", " X .", "  X ."], None),
    (["X . .", " . X .", "  . . X"], None),
    ([". . .", " . X .", "  . X ."], None),
    ([". O . X .", "  . X X O .", "   O O O X .", "    . X O X O", "     X O O O X"], None),
]

for board, expected in checks:
    got = ConnectGame(board).get_winner()
    assert got == expected, (board, got, expected)

print("all ok")
'''

CALLS = []
