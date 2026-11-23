python3 - <<'PY'
from bowling import BowlingGame


def game(rolls):
    g = BowlingGame()
    for r in rolls:
        g.roll(r)
    return g


def check(name, got, want):
    assert got == want, f"{name}: got {got}, want {want}"
    print("ok", name)


check("gutter", game([0] * 20).score(), 0)
check("all ones", game([1] * 20).score(), 20)
check("one spare", game([5, 5, 3] + [0] * 17).score(), 16)
check("one strike", game([10, 3, 4] + [0] * 16).score(), 24)
check("perfect", game([10] * 12).score(), 300)
check("all spares", game([5] * 21).score(), 150)
check("tenth spare+fill", game([0] * 18 + [10, 1, 9]).score(), 20)
check("tenth triple strike", game([0] * 18 + [10, 10, 10]).score(), 30)
check("tenth open", game([0] * 18 + [9, 0]).score(), 9)
check("spare then strike", game([5, 5, 10] + [0] * 16).score(), 20 + 10)

def raises(name, exc, fn):
    try:
        fn()
    except exc as e:
        assert str(e), f"{name}: empty message"
        print("ok", name, "->", type(e).__name__, str(e))
    else:
        raise AssertionError(f"{name}: no exception raised")


raises("negative", ValueError, lambda: BowlingGame().roll(-1))
raises("too many pins", ValueError, lambda: BowlingGame().roll(11))
raises("frame over ten", ValueError, lambda: game([5]).roll(6))
raises("tenth open bonus", IndexError, lambda: game([0] * 18 + [9, 0]).roll(10))
raises("incomplete score", IndexError, lambda: game([5]).score())
raises("extra roll after open game", IndexError, lambda: game([0] * 20).roll(0))
raises("after game over", ValueError, lambda: game([0] * 18 + [10, 0, 0]).roll(0))
raises("fourth fill ball", ValueError, lambda: game([0] * 18 + [10, 10, 10]).roll(0))
raises("over-ten fill", ValueError, lambda: game([0] * 18 + [10, 5]).roll(7))
print("ALL PASS")
PY
