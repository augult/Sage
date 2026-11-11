"""Promotion codes: a percentage off some SKUs."""

from pricing import catalogue


def apply(prices, code):
    """The price table with the promotion applied, rounded down to a cent. Unknown code: ValueError."""
    promos = catalogue()["promos"]
    if code not in promos:
        raise ValueError(f"unknown promotion: {code}")
    promotion = promos[code]
    for sku in promotion["skus"]:
        prices[sku] = prices[sku] * (100 - promotion["percent"]) // 100
    return prices
