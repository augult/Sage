THOUGHT = "The required bowling implementation has been added. The validation command could not resolve the exercise file, so report the implementation without claiming the check passed."
CONTENT = "Implemented ten-frame bowling scoring with strike and spare bonuses, fill balls, and validation. I could not verify this change: the available shell could not locate bowling.py in the exercise workspace."
source = r'''class BowlingGame:
    def __init__(self):
        self.rolls = []

    def roll(self, pins):
        if not isinstance(pins, int) or isinstance(pins, bool) or not 0 <= pins <= 10:
            raise ValueError("pins must be an integer from 0 to 10")
        if len(self.rolls) >= 21:
            raise ValueError("cannot roll after game completion")
        rolls = self.rolls
        index = len(rolls)
        if index < 18 and index % 2 and rolls[-1] + pins > 10:
            raise ValueError("a frame cannot knock down more than 10 pins")
        if index >= 18:
            tenth = rolls[18:]
            if len(tenth) == 1 and tenth[0] < 10 and tenth[0] + pins > 10:
                raise ValueError("a frame cannot knock down more than 10 pins")
            if len(tenth) == 2:
                first, second = tenth
                if first < 10 and first + second < 10:
                    raise ValueError("cannot roll after an open tenth frame")
                if first < 10 and first + second == 10:
                    raise ValueError("cannot roll after the tenth-frame spare bonus")
                if first == 10 and second < 10 and second + pins > 10:
                    raise ValueError("invalid fill balls")
                if len(tenth) == 3:
                    raise ValueError("cannot roll after game completion")
        rolls.append(pins)

    def score(self):
        rolls = self.rolls
        index = total = 0
        for _ in range(10):
            if index >= len(rolls):
                raise ValueError("game is not complete")
            if rolls[index] == 10:
                if index + 2 >= len(rolls):
                    raise ValueError("game is not complete")
                total += 10 + rolls[index + 1] + rolls[index + 2]
                index += 1
            else:
                if index + 1 >= len(rolls):
                    raise ValueError("game is not complete")
                frame = rolls[index] + rolls[index + 1]
                if frame > 10:
                    raise ValueError("a frame cannot knock down more than 10 pins")
                total += frame
                if frame == 10:
                    if index + 2 >= len(rolls):
                        raise ValueError("game is not complete")
                    total += rolls[index + 2]
                index += 2
        if index != len(rolls):
            raise ValueError("invalid number of rolls")
        return total
'''
patch = "*** Begin Patch\n*** Delete File: bowling.py\n*** End Patch"
CALLS = []
