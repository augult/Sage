class BowlingError(ValueError, IndexError):
    """Raised when a roll or a score request breaks the rules of bowling."""


class BowlingGame:
    """Track the rolls of one ten-pin bowling game and score it."""

    def __init__(self):
        self.rolls = []

    def roll(self, pins):
        if not isinstance(pins, int) or not 0 <= pins <= 10:
            raise BowlingError(
                f"invalid roll {pins!r}: a roll must knock down between 0 and 10 pins"
            )
        self.rolls.append(pins)
        try:
            self._frames()
        except BowlingError:
            self.rolls.pop()
            raise

    def score(self):
        frames = self._frames(require_complete=True)
        total = 0
        for start, _, kind in frames[:9]:
            if kind == "strike":
                total += 10 + self.rolls[start + 1] + self.rolls[start + 2]
            elif kind == "spare":
                total += 10 + self.rolls[start + 2]
            else:
                total += self.rolls[start] + self.rolls[start + 1]
        start, used, _ = frames[9]
        return total + sum(self.rolls[start : start + used])

    def _frames(self, require_complete=False):
        """Split the recorded rolls into frames, validating the rules.

        Returns ``(start, roll_count, kind)`` per frame played so far and raises
        ``BowlingError`` when the rolls break a rule.
        """
        rolls = self.rolls
        frames = []
        index = 0

        for number in range(1, 10):
            if index >= len(rolls):
                break
            if rolls[index] == 10:
                frames.append((index, 1, "strike"))
                index += 1
                continue
            if index + 1 >= len(rolls):
                break
            pins = rolls[index] + rolls[index + 1]
            if pins > 10:
                raise BowlingError(
                    f"frame {number} totals {pins} pins; a frame cannot score more than 10"
                )
            frames.append((index, 2, "spare" if pins == 10 else "open"))
            index += 2

        complete = False
        if len(frames) == 9 and index < len(rolls):
            start = index
            first = rolls[start]
            second = rolls[start + 1] if start + 1 < len(rolls) else None
            third = rolls[start + 2] if start + 2 < len(rolls) else None

            if first == 10:
                required = 3
                if second is not None and second != 10:
                    if third is not None and (third == 10 or second + third > 10):
                        raise BowlingError(
                            "the bonus rolls of the tenth frame cannot total more "
                            "than 10 pins, and the second bonus roll cannot be a "
                            "strike when the first one is not"
                        )
            elif second is None:
                required = 2
            else:
                pins = first + second
                if pins > 10:
                    raise BowlingError(
                        f"the tenth frame totals {pins} pins; a frame cannot score more than 10"
                    )
                if pins == 10:
                    required = 3
                else:
                    required = 2
                    if third is not None:
                        raise BowlingError(
                            "an open tenth frame does not allow a third roll"
                        )

            used = min(len(rolls) - start, required)
            frames.append((start, used, "tenth"))
            complete = used == required
            index = start + used

        if index < len(rolls):
            raise BowlingError("cannot roll: the game already has ten frames")

        if require_complete and not complete:
            raise BowlingError(
                "cannot score: all ten frames must be rolled before scoring"
            )

        return frames
