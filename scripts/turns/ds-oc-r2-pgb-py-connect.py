THOUGHT = r'''Run a suite of boards covering both directions, zigzags and non-connections.'''

CONTENT = '''Running the sanity checks.'''

TEST = r'''from connect import ConnectGame

checks = [
    ([". . .", " . . .", "  . . ."], None),
    (["X X X"], "X"),
    (["O", " O", "  O"], "O"),
    (["O . .", " . . .", "  . . ."], None),
    (["X . .", " X .", "  X ."], None),
    (["X . .", " . X .", "  . . X"], None),
    ([". . .", " X X X", "  . . .", "   . . ."], None),
    ([". O . X .", "  . X X O .", "   O O O X .", "    . X O X O", "     X O O O X"], None),
]

for board, expected in checks:
    got = ConnectGame(board).get_winner()
    assert got == expected, (board, got, expected)

print("all ok")
'''

CALLS = [
    {"name": "bash", "arguments": {"command": "python3 -c '" + TEST + "'"}},
]
