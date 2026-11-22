THOUGHT = "Implement standard bowling scoring and run focused scoring checks."
CONTENT = ""
CALLS = [
    {"name": "edit", "arguments": {"path": "bowling.py", "range": "1yb-9ke", "new": r'''class BowlingGame:
    def __init__(self):
        self.rolls = []

    def roll(self, pins):
        if not isinstance(pins, int) or isinstance(pins, bool) or not 0 <= pins <= 10:
            raise ValueError("pins must be an integer from 0 to 10")
        if len(self.rolls) >= 20:
            raise ValueError("game is complete")
        if len(self.rolls) < 18 and len(self.rolls) % 2 and self.rolls[-1] != 10 and self.rolls[-1] + pins > 10:
            raise ValueError("a frame cannot knock down more than 10 pins")
        if len(self.rolls) >= 18:
            frame = self.rolls[18:]
            if len(frame) == 1 and frame[0] < 10 and frame[0] + pins > 10:
                raise ValueError("a frame cannot knock down more than 10 pins")
            if len(frame) == 2:
                if frame[0] == 10 and frame[1] < 10 and frame[1] + pins > 10:
                    raise ValueError("fill balls cannot knock down more than 10 pins")
                if frame[0] < 10 and frame[0] + frame[1] < 10:
                    raise ValueError("game is complete")
        self.rolls.append(pins)

    def score(self):
        i = total = 0
        for _ in range(10):
            if i >= len(self.rolls):
                raise ValueError("game is not complete")
            if self.rolls[i] == 10:
                if i + 2 >= len(self.rolls):
                    raise ValueError("game is not complete")
                total += 10 + self.rolls[i + 1] + self.rolls[i + 2]
                i += 1
            else:
                if i + 1 >= len(self.rolls):
                    raise ValueError("game is not complete")
                frame = self.rolls[i] + self.rolls[i + 1]
                if frame > 10:
                    raise ValueError("a frame cannot knock down more than 10 pins")
                total += frame
                if frame == 10:
                    if i + 2 >= len(self.rolls):
                        raise ValueError("game is not complete")
                    total += self.rolls[i + 2]
                i += 2
        if i != len(self.rolls):
            raise ValueError("invalid number of rolls")
        return total'''}},
    {"name": "shell", "arguments": {"cmd": "python3 -m py_compile bowling.py && python3 - <<'PY'\nfrom bowling import BowlingGame\ndef score(rs):\n g=BowlingGame()\n for r in rs:g.roll(r)\n return g.score()\nassert score([0,0]*10)==0\nassert score([10,3,4]+[0,0]*8)==24\nassert score([10]*12)==300\nassert score([10]*9+[10,1,9])==281\nPY", "expect_exit": "0", "timeout": 30}},
    {"name": "finish", "arguments": {"summary": "Implemented bowling frame and bonus scoring, tenth-frame fill balls, roll validation, and incomplete-game errors. Compilation and focused score checks pass.", "unverified": "Hidden grader tests are unavailable."}}
]
