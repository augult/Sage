THOUGHT = r'''The stub is a `ConnectGame(board)` class with a `get_winner()` method, so the implementation must live there (the module-level helper can stay for convenience). My "o straight row" expectation was simply wrong: a single row of O does not connect top to bottom.'''

CONTENT = r'''The stub exposes `ConnectGame(board).get_winner()`, so I'll put the solver in that class and keep a module-level convenience wrapper.'''

CALLS = [
    {"name": "bash", "arguments": {"command": r'''cat > connect.py <<'EOF'
"""Compute the winner of a game of Hex / CON-TAC-TIX."""

from collections import deque

# Rows are indented one extra space per row, so a cell's four diagonal
# neighbours are (r-1, c), (r-1, c+1), (r+1, c-1) and (r+1, c).
NEIGHBOURS = ((0, -1), (0, 1), (-1, 0), (-1, 1), (1, -1), (1, 0))


class ConnectGame:
    """A Hex / Polygon board."""

    def __init__(self, board):
        if isinstance(board, str):
            board = board.splitlines()
        self.board = board
        self.cells = [line.split() for line in board]

    def get_winner(self):
        """Return "X", "O", or "" when neither player has connected."""
        if not self.cells:
            return ""

        rows = self.cells
        left = [(r, 0) for r in range(len(rows))]
        top = [(0, c) for c in range(len(rows[0]))]

        if self._connects(left, "X", lambda r, c: c == len(rows[r]) - 1):
            return "X"
        if self._connects(top, "O", lambda r, c: r == len(rows) - 1):
            return "O"
        return ""

    def _connects(self, starts, player, reached):
        rows = self.cells
        seen = set()
        queue = deque()
        for r, c in starts:
            if 0 <= r < len(rows) and 0 <= c < len(rows[r]) and rows[r][c] == player:
                seen.add((r, c))
                queue.append((r, c))

        while queue:
            r, c = queue.popleft()
            if reached(r, c):
                return True
            for dr, dc in NEIGHBOURS:
                nr, nc = r + dr, c + dc
                if not (0 <= nr < len(rows) and 0 <= nc < len(rows[nr])):
                    continue
                if (nr, nc) in seen or rows[nr][nc] != player:
                    continue
                seen.add((nr, nc))
                queue.append((nr, nc))
        return False


def get_winner(board):
    return ConnectGame(board).get_winner()


winner = get_winner
EOF
python3 - <<'EOF'
from connect import ConnectGame, get_winner

def check(name, board, want):
    got = ConnectGame(board).get_winner()
    assert got == want, f"{name}: got {got!r} want {want!r}"
    assert get_winner(board) == got
    print(f"ok {name}: {got!r}")

check("empty-ish", [". . .", " . . .", "  . . ."], "")
check("x win", ["X X .", ". X X", ". . X"], "X")
check("o top-bottom", ["O . .", "O . .", "O . ."], "O")
check("o single row", ["O O O", " . . .", "  . . ."], "")
check("x single row", ["X X X", " . . .", "  . . ."], "X")
check("readme board", [
    ". O . X .",
    " . X X O .",
    "  O O O X .",
    "   . X O X O",
    "    X O O O X",
], "")
check("x column only", ["X . .", "X . .", "X . ."], "")
check("no diagonal", ["X X . .", " . . . ."], "")
check("x off grid", ["X . .", " X .", "  . ."], "")
check("string board", "X . .\n . . .\n  . . .", "")

# X winding right through the hex rows still wins
check("x winding", [
    "X X . . .",
    " . X X . .",
    "  . . X X .",
    "   . . . X X",
    "    . . . . X",
], "X")
print("all passed")
EOF'''}},
]
