THOUGHT = "Inspect the repo, read README.md, and read the stub to match the expected API."

THOUGHT = "Flood-fill empty regions and attribute each one to the single colour bordering it; regions touching both colours (or none) are neutral ''."

CONTENT = "Implemented Board in go_counting.py: flood-fills each empty region, assigns it to the single colour bordering it, treats mixed/none as neutral '', and raises ValueError('Invalid coordinate') for off-board coordinates. Verified with the 5x5 example board, an all-black board, an empty board and off-board coordinate errors."

SOURCE = r'''
class Board:
    """Count territories of each player in a Go game."""

    def __init__(self, board):
        self.board = [list(row) for row in board]

    def _validate(self, x, y):
        if y < 0 or y >= len(self.board) or x < 0 or x >= len(self.board[y]):
            raise ValueError("Invalid coordinate")

    def _region(self, x, y):
        cells = {(x, y)}
        borders = set()
        stack = [(x, y)]
        while stack:
            cx, cy = stack.pop()
            for nx, ny in ((cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)):
                if 0 <= ny < len(self.board) and 0 <= nx < len(self.board[ny]):
                    value = self.board[ny][nx]
                    if value == " ":
                        if (nx, ny) not in cells:
                            cells.add((nx, ny))
                            stack.append((nx, ny))
                    else:
                        borders.add(value)
        return borders, cells

    def territory(self, x, y):
        """Return (owner, territory) for the region containing x, y."""
        self._validate(x, y)
        if self.board[y][x] != " ":
            return "", set()
        borders, cells = self._region(x, y)
        if len(borders) == 1:
            return borders.pop(), cells
        return "", set()

    def territories(self):
        """Return {owner: set of coordinates} for the whole board."""
        result = {"B": set(), "W": set(), "": set()}
        seen = set()
        for y, row in enumerate(self.board):
            for x, value in enumerate(row):
                if value != " " or (x, y) in seen:
                    continue
                borders, cells = self._region(x, y)
                seen |= cells
                owner = borders.pop() if len(borders) == 1 else ""
                result[owner] |= cells
        return result
'''

TEST = r'''
from go_counting import Board

b = Board(["  B  ", " B B ", "B W B", " W W ", "  W  "])
print(b.territories())
print(b.territory(0, 0))

black = Board(["B B", " B "])
assert black.territories() == {"B": {(1, 0), (0, 1), (2, 1)}, "W": set(), "": set()}
assert black.territory(1, 0) == ("B", {(1, 0)})

empty = Board([" "])
assert empty.territories() == {"B": set(), "W": set(), "": {(0, 0)}}
assert empty.territory(0, 0) == ("", set())

neutral = Board([" B ", "B  ", "BW "])
assert neutral.territory(1, 1) == ("", set()), neutral.territory(1, 1)

for x, y in ((-1, 0), (0, -1), (3, 0), (0, 3)):
    try:
        neutral.territory(x, y)
    except ValueError as err:
        assert str(err) == "Invalid coordinate", err
    else:
        raise AssertionError((x, y))
print("ok")
'''

CALLS = []
