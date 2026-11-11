"""Postal code to zone (README rules 1 and 2)."""

from . import rates
from .errors import QuoteError


def normalise(postal_code):
    """The postal code without spaces, in upper case."""
    raise NotImplementedError


def lookup(postal_code):
    """`(zone, remote)` for a postal code as the customer typed it."""
    raise NotImplementedError
