"""Postal code to zone (README rules 1 and 2)."""

from . import rates
from .errors import QuoteError


def normalise(postal_code):
    """The postal code without spaces, in upper case."""
    return "".join(str(postal_code).split()).upper()


def lookup(postal_code):
    """`(zone, remote)` for a postal code as the customer typed it."""
    code = normalise(postal_code)
    best = None
    for prefix, zone, remote in rates.PREFIXES:
        if code.startswith(prefix) and (best is None or len(prefix) > len(best[0])):
            best = (prefix, zone, remote)
    if best is None:
        raise QuoteError(f"unknown destination: {postal_code!r}")
    return best[1], best[2]
