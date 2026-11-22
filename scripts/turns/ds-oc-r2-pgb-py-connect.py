THOUGHT = r'''Verify with the read tool whether the write landed.'''

CONTENT = '''Checking the file contents.'''

CODE = r'''class ConnectGame:
    """Determine the winner of a game of Hex (CON-TAC-TIX)."""

    def __init__(self, board):
        self.rows = []
        for line in board:
            cells = line.split()
            for cell in cells:
                if cell not in (".", "O", "X"):
                    raise ValueError(f"invalid board character: {cell!r}")
            self.rows.append(cells)

    def get_winner(self):
        if self._wins("O", vertical=True):
            return "O"
        if self._wins("X", vertical=False):
            return "X"
        return None

    def _neighbours(self, row, col):
        candidates = (
            (row, col - 1),
            (row, col + 1),
            (row - 1, col),
            (row - 1, col + 1),
            (row + 1, col),
            (row + 1, col - 1),
        )
        for r, c in candidates:
            if 0 <= r < len(self.rows) and 0 <= c < len(self.rows[r]):
                yield r, c

    def _wins(self, player, vertical):
        if not self.rows:
            return False
        width = max(len(row) for row in self.rows)
        last_row = len(self.rows) - 1

        # O starts on the top row and must reach the bottom row.
        # X starts on the left column and must reach the right column.
        if vertical:
            starts = [(0, c) for c, cell in enumerate(self.rows[0]) if cell == player]
        else:
            starts = [(r, 0) for r, row in enumerate(self.rows) if row and row[0] == player]

        seen = set(starts)
        stack = list(starts)
        while stack:
            r, c = stack.pop()
            if vertical and r == last_row:
                return True
            if not vertical and c == width - 1:
                return True
            for nr, nc in self._neighbours(r, c):
                if (nr, nc) not in seen and self.rows[nr][nc] == player:
                    seen.add((nr, nc))
                    stack.append((nr, nc))
        return False
'''

CALLS = [
    {"name": "read", "arguments": {"filePath": "connect.py"}},
]

CALLS = [
    {"name": "read", "arguments": {"filePath": "connect.py"}},
]
