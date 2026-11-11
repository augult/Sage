"""Bundles: several SKUs sold together for 5% less than their sum."""

from pricing import catalogue

BUNDLE_DISCOUNT_PCT = 5


def with_bundles(prices):
    """The price table with an entry for every bundle, rounded down to a cent."""
    for name, parts in catalogue()["bundles"].items():
        prices[name] = sum(prices[part] for part in parts) * (100 - BUNDLE_DISCOUNT_PCT) // 100
    return prices
