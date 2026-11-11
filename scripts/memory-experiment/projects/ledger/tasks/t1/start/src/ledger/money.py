"""Amounts are integer cents."""


def parse_amount(text: str) -> int:
    """'12.50' -> 1250, '-3' -> -300, '0.5' -> 50."""
    text = text.strip()
    sign = -1 if text.startswith("-") else 1
    text = text.lstrip("+-")
    whole, _, frac = text.partition(".")
    if not whole.isdigit() or (frac and not frac.isdigit()) or len(frac) > 2:
        raise ValueError(f"bad amount: {text!r}")
    return sign * (int(whole) * 100 + int(frac.ljust(2, "0")))


def format_cents(cents: int) -> str:
    """1250 -> '12.50'."""
    sign = "-" if cents < 0 else ""
    whole, frac = divmod(abs(cents), 100)
    return f"{sign}{whole}.{frac:02d}"
