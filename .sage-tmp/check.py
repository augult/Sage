import sys

sys.path.insert(0, ".")

from bowling import BowlingGame


def new_game(rolls):
    game = BowlingGame()
    for pins in rolls:
        game.roll(pins)
    return game


def failure(label, message):
    raise AssertionError(f"{label}: {message}")


def expect_error(label, action):
    try:
        action()
    except ValueError as err:
        if not str(err):
            failure(label, "raised without a message")
        return
    except Exception as err:  # noqa: BLE001
        failure(label, f"raised {type(err).__name__} instead of ValueError: {err}")
    failure(label, "no error was raised")


scores = [
    ("all zeros", [0] * 20, 0),
    ("all threes and sixes", [3, 6] * 10, 90),
    ("spare then zeros", [6, 4] + [0] * 18, 10),
    ("spare bonus counted once", [6, 4, 3] + [0] * 17, 16),
    ("strike bonus", [10, 5, 3] + [0] * 16, 26),
    ("readme example", [10, 5, 5, 9, 0] + [0] * 14, 48),
    ("all spares", [5, 5] * 10 + [5], 150),
    ("all nines and spares", [9, 1] * 10 + [9], 190),
    ("perfect game", [10] * 12, 300),
    ("eleven strikes", [10] * 11 + [9], 299),
    ("strike then spare", [10, 5, 5] + [0] * 16, 30),
    ("tenth frame spare", [0] * 18 + [7, 3, 10], 20),
    ("tenth frame strike", [0] * 18 + [10, 7, 1], 18),
    ("tenth frame three strikes", [0] * 18 + [10, 10, 10], 30),
]

for label, rolls, expected in scores:
    game = new_game(rolls)
    got = game.score()
    if got != expected:
        failure(label, f"expected {expected}, got {got}")
    if game.score() != expected:
        failure(label, "score() is not stable across calls")

expect_error("roll above ten", lambda: BowlingGame().roll(11))
expect_error("negative roll", lambda: BowlingGame().roll(-1))
expect_error("frame above ten", lambda: new_game([5, 6]))
expect_error("tenth fill above ten", lambda: new_game([0] * 18 + [10, 5, 6]))
expect_error("tenth second fill is strike", lambda: new_game([0] * 18 + [10, 6, 10]))
expect_error("tenth zero then strike fill", lambda: new_game([0] * 18 + [10, 0, 10]))
expect_error("tenth open frame above ten", lambda: new_game([0] * 18 + [7, 4]))
expect_error("roll after open tenth frame", lambda: new_game([0] * 18 + [9, 0, 5]))
expect_error("roll after ten frames", lambda: new_game([0] * 20).roll(0))
expect_error("score before rolling", lambda: BowlingGame().score())
expect_error("score with incomplete tenth", lambda: new_game([0] * 19).score())
expect_error("score with spare lacking fill", lambda: new_game([0] * 18 + [7, 3]).score())
expect_error("score with strike lacking fills", lambda: new_game([0] * 18 + [10, 10]).score())

game = new_game([0] * 18 + [10, 10])
before = list(game.rolls)
expect_error("invalid bonus after two strikes", lambda: game.roll(11))
if game.rolls != before:
    failure("state after failed roll", "rolls changed")
game.roll(10)
if game.score() != 30:
    failure("state after failed roll", f"score is {game.score()}, expected 30")

game = new_game([0] * 20)
before = list(game.rolls)
expect_error("extra roll after complete game", lambda: game.roll(0))
if game.rolls != before:
    failure("state after rejected extra roll", "rolls changed")
if game.score() != 0:
    failure("score after rejected extra roll", f"score is {game.score()}")

print("ALL OK")
