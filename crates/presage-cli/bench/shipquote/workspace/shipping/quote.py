"""The quote for an order (README rules 5 to 8 and 12)."""

from . import rates, surcharges, weights, zones
from .errors import QuoteError


def quote(order):
    """The quote for `order`, as described in the README."""
    raise NotImplementedError
