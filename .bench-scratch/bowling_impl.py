class IllegalRollError(IndexError, ValueError):
    """A roll that breaks the rules of the game."""


class GameIncompleteError(IndexError, ValueError):
    """score() was called before the game had ten complete frames."""


class BowlingGame:
    """Score a ten-pin bowling game."""

    FRAMES = 10

    def __init__(self):
        self._frames = []

    def roll(self, pins):
        if isinstance(pins, bool) or not isinstance(pins, int):
            raise ValueError("pins must be an integer")
        if pins < 0 or pins > 10:
            raise ValueError("pins must be between 0 and 10")
        if len(self._frames) == self.FRAMES:
            tenth = self._frames[-1]
            if len(tenth) == 2 and tenth[0] != 10 and sum(tenth) < 10:
                raise IllegalRollError("cannot throw bonus with an open tenth frame")
        if self._is_complete():
            raise ValueError("cannot roll after the game is over")

        if not self._frames or self._frame_finished(self._frames[-1], len(self._frames)):
            self._frames.append([])
        frame = self._frames[-1]
        number = len(self._frames)

        if number < self.FRAMES:
            if frame and frame[0] + pins > 10:
                raise ValueError("two rolls in a frame cannot exceed ten pins")
        elif len(frame) == 1 and frame[0] != 10:
            if frame[0] + pins > 10:
                raise ValueError("two rolls in a frame cannot exceed ten pins")
        elif len(frame) == 2:
            if frame[0] == 10:
                if frame[1] != 10 and frame[1] + pins > 10:
                    raise ValueError("fill balls cannot exceed ten pins")

        frame.append(pins)

    def score(self):
        if not self._is_complete():
            raise GameIncompleteError("score cannot be called until the game is complete")

        rolls = [pins for frame in self._frames for pins in frame]
        total = 0
        index = 0
        for _ in range(self.FRAMES):
            if rolls[index] == 10:
                total += 10 + rolls[index + 1] + rolls[index + 2]
                index += 1
            elif rolls[index] + rolls[index + 1] == 10:
                total += 10 + rolls[index + 2]
                index += 2
            else:
                total += rolls[index] + rolls[index + 1]
                index += 2
        return total

    def _is_complete(self):
        if len(self._frames) < self.FRAMES:
            return False
        tenth = self._frames[-1]
        if len(tenth) >= 3:
            return True
        return len(tenth) == 2 and tenth[0] != 10 and sum(tenth) < 10

    def _frame_finished(self, frame, number):
        if number < self.FRAMES:
            return len(frame) == 2 or frame[0] == 10
        return self._is_complete() or (
            len(frame) == 2 and frame[0] != 10 and sum(frame) < 10
        )
