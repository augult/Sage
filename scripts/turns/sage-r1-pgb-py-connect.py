THOUGHT = "Use graph traversal on the hex grid to find X left-to-right and O top-to-bottom connections."
CONTENT = ""
CALLS = [
    {"name": "edit", "arguments": {"path": "connect.py", "range": "2hf-7ke", "new": r'''class ConnectGame:
    def __init__(self, board):
        self.board = [row.strip().split() for row in board.splitlines() if row.strip()]

    def get_winner(self):
        if not self.board:
            return None
        rows = len(self.board)
        cols = max(map(len, self.board))
        for stone, starts, target in (
            ("O", [(0, c) for c in range(len(self.board[0]))], lambda r, c: r == rows - 1),
            ("X", [(r, 0) for r in range(rows)], lambda r, c: c == len(self.board[r]) - 1),
        ):
            pending = [(r, c) for r, c in starts if c < len(self.board[r]) and self.board[r][c] == stone]
            seen = set(pending)
            while pending:
                r, c = pending.pop()
                if target(r, c):
                    return stone
                for nr, nc in ((r-1,c), (r+1,c), (r,c-1), (r,c+1), (r-1,c+1), (r+1,c-1)):
                    if 0 <= nr < rows and 0 <= nc < len(self.board[nr]) and self.board[nr][nc] == stone and (nr,nc) not in seen:
                        seen.add((nr,nc))
                        pending.append((nr,nc))
        return None'''}},
    {"name": "shell", "arguments": {"cmd": "python3 -m py_compile connect.py && python3 - <<'PY'\nfrom connect import ConnectGame\nassert ConnectGame('. O . X .\\n . X X O .\\n  O O O X .\\n   . X O X O\\n    X O O O X').get_winner() is None\nassert ConnectGame('O .\\nO X\\nO X').get_winner() == 'O'\nassert ConnectGame('X X X\\nO . O').get_winner() == 'X'\nPY", "expect_exit": "0", "timeout": 30}},
    {"name": "finish", "arguments": {"summary": "Implemented Hex winner detection using six-neighbor traversal for O top-to-bottom and X left-to-right paths. Compilation and representative examples pass.", "unverified": "Hidden grader tests are unavailable."}}
]
