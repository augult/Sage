THOUGHT = r'''I need to see the task requirements first. Reading README.md and listing the workspace.'''

CONTENT = '''Let me start by reading the task description.'''

THOUGHT = r'''Implement bowling scoring with strike/spare bonuses and validation for invalid frames and fill balls.'''

CONTENT = '''I'll implement the scorer now.'''

CODE = r'''class BowlingGame:
    """Score a single ten-pin bowling game."""

    def __init__(self):
        self.rolls = []

    def roll(self, pins):
        if not isinstance(pins, int) or isinstance(pins, bool):
            raise TypeError("pins must be an integer")
        if pins < 0 or pins > 10:
            raise ValueError(f"invalid number of pins: {pins}")
        self.rolls.append(pins)
        self._validate()

    def _validate(self):
        rolls = self.rolls
        i = 0
        for frame in range(1, 10):
            if i >= len(rolls):
                return
            if rolls[i] == 10:
                i += 1
                continue
            if i + 1 >= len(rolls):
                return
            if rolls[i] + rolls[i + 1] > 10:
                raise ValueError(f"frame {frame} total exceeds 10 pins")
            i += 2

        # Tenth frame: two throws, plus fill balls only after a strike or spare.
        if i >= len(rolls):
            return
        first = rolls[i]
        if first == 10:
            if len(rolls) > i + 3:
                raise ValueError("cannot throw bonus with an open tenth frame")
            return
        if i + 1 >= len(rolls):
            return
        second = rolls[i + 1]
        if first + second > 10:
            raise ValueError("frame 10 total exceeds 10 pins")
        if first + second < 10:
            if len(rolls) > i + 2:
                raise ValueError("cannot throw bonus with an open tenth frame")
        elif len(rolls) > i + 3:
            raise ValueError("invalid fill balls")

    def score(self):
        total = 0
        i = 0
        for _ in range(10):
            if i >= len(self.rolls):
                break
            if self.rolls[i] == 10:
                total += 10 + sum(self.rolls[i + 1:i + 3])
                i += 1
            elif i + 1 >= len(self.rolls):
                total += self.rolls[i]
                i += 1
            elif self.rolls[i] + self.rolls[i + 1] == 10:
                bonus = self.rolls[i + 2] if i + 2 < len(self.rolls) else 0
                total += 10 + bonus
                i += 2
            else:
                total += self.rolls[i] + self.rolls[i + 1]
                i += 2
        return total
'''

CALLS = [
    {"name": "write", "arguments": {"filePath": "bowling.py", "content": CODE}},
    {"name": "bash", "arguments": {"command": "(head -5 bowling.py; python3 - <<'EOF'\nfrom bowling import BowlingGame\n\ndef g(rs):\n    b = BowlingGame()\n    for r in rs: b.roll(r)\n    return b.score()\n\nassert g([0]*20) == 0\nassert g([5]*21) == 150\nassert g([10]*12) == 300\nassert g([1,4,4,5,6,4,5,5,10,0,1,7,3,6,4,10,2,8,6]) == 133\nassert g([9,0]*10) == 90\nprint('all ok')\nEOF\n)"}},
]
