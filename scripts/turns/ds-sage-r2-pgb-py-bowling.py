THOUGHT = r'''The README asks for a `BowlingGame` with `roll`/`score`, the standard ten-frame rules with tenth-frame fill balls, and meaningful `ValueError` messages for invalid play. I'll implement frames explicitly so per-frame and fill-ball validation happens at roll time, then check scoring and error cases.'''
CONTENT = ""
CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "bowling.py",
            "overwrite": True,
            "content": r'''class BowlingGame:
    MAX_PINS = 10
    MAX_FRAMES = 10

    def __init__(self):
        self._frames = []
        self._current = []
        self._finished = False

    def roll(self, pins):
        if not isinstance(pins, int) or isinstance(pins, bool):
            raise ValueError("pins must be an integer")
        if pins < 0 or pins > self.MAX_PINS:
            raise ValueError("pins must be between 0 and 10")
        if self._finished:
            raise ValueError("cannot roll after the game is complete")

        if len(self._frames) < self.MAX_FRAMES - 1:
            if self._current and self._current[0] + pins > self.MAX_PINS:
                raise ValueError("a frame cannot score more than 10 pins")
            self._current.append(pins)
            if pins == self.MAX_PINS or len(self._current) == 2:
                self._frames.append(self._current)
                self._current = []
            return

        self._roll_tenth(pins)

    def _roll_tenth(self, pins):
        frame = self._current
        if not frame:
            self._current.append(pins)
            return

        first = frame[0]
        if len(frame) == 1:
            if first != self.MAX_PINS and first + pins > self.MAX_PINS:
                raise ValueError("a frame cannot score more than 10 pins")
            self._current.append(pins)
            if first != self.MAX_PINS and first + pins < self.MAX_PINS:
                self._finished = True
            return

        second = frame[1]
        if first == self.MAX_PINS and second != self.MAX_PINS:
            if second + pins > self.MAX_PINS:
                raise ValueError("fill balls cannot score more than 10 pins")
        self._current.append(pins)
        self._finished = True

    def _all_rolls(self):
        rolls = [pin for frame in self._frames for pin in frame]
        rolls.extend(self._current)
        return rolls

    def score(self):
        rolls = self._all_rolls()
        total = 0
        index = 0
        for _ in range(self.MAX_FRAMES):
            if index >= len(rolls):
                break
            if rolls[index] == self.MAX_PINS:
                total += self.MAX_PINS + sum(rolls[index + 1:index + 3])
                index += 1
            elif index + 1 < len(rolls) and rolls[index] + rolls[index + 1] == self.MAX_PINS:
                total += self.MAX_PINS + (rolls[index + 2] if index + 2 < len(rolls) else 0)
                index += 2
            else:
                total += sum(rolls[index:index + 2])
                index += 2
        return total
''',
        },
    },
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''python3 - <<'PY'
import sys
sys.path.insert(0, ".")
from bowling import BowlingGame

def game(rolls):
    g = BowlingGame()
    for r in rolls:
        g.roll(r)
    return g

def bad(rolls, extra):
    g = game(rolls)
    try:
        g.roll(extra)
    except ValueError as exc:
        assert str(exc), "error message must not be empty"
        return
    raise AssertionError("expected ValueError for %r + %r" % (rolls, extra))

cases = [
    ([], 0),
    ([0] * 20, 0),
    ([1] * 20, 20),
    ([9, 0] * 10, 90),
    ([5, 5] * 10 + [5], 150),
    ([10] * 12, 300),
    ([0] * 18 + [10, 10, 10], 30),
    ([0] * 18 + [7, 3, 4], 14),
    ([1, 4, 4, 5, 6, 4, 5, 5, 10, 0, 1, 7, 3, 6, 4, 10, 2, 8, 6], 133),
    ([10, 5, 5] + [0] * 16, 30),
]
for rolls, want in cases:
    got = game(rolls).score()
    assert got == want, (rolls, got, want)

bad([], -1)
bad([], 11)
bad([5], 6)
bad([6], 5)
bad([0] * 18 + [5], 6)
bad([0] * 18 + [10, 5], 6)
bad([10] * 12, 0)
bad([0] * 20, 0)
bad([5, 5] * 10 + [5], 5)
print("ALL PASS")
PY''',
            "expect_exit": "0",
            "expect_has": "ALL PASS",
        },
    },
]
