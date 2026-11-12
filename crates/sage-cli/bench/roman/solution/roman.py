"""Roman numerals, 1 to 3999."""

_SYMBOLS = [
    (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
    (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
]


def to_roman(n):
    """The Roman numeral for an integer from 1 to 3999."""
    if isinstance(n, bool) or not isinstance(n, int) or not 1 <= n <= 3999:
        raise ValueError(f"not a number from 1 to 3999: {n!r}")
    out = []
    for value, symbol in _SYMBOLS:
        while n >= value:
            out.append(symbol)
            n -= value
    return "".join(out)


def from_roman(s):
    """The integer a correctly formed Roman numeral stands for."""
    if not isinstance(s, str) or not s:
        raise ValueError(f"not a Roman numeral: {s!r}")
    total, rest = 0, s
    for value, symbol in _SYMBOLS:
        while rest.startswith(symbol):
            total += value
            rest = rest[len(symbol):]
    # Only the canonical spelling of a number is a correctly formed numeral.
    if rest or not 1 <= total <= 3999 or to_roman(total) != s:
        raise ValueError(f"not a Roman numeral: {s!r}")
    return total
