"""VAT."""

from pricing import vat_pct


def add_vat(cents, region):
    """The amount with the region's VAT added, rounded to a cent with halves up."""
    return (cents * (100 + vat_pct(region)) + 50) // 100


def gross(prices, region):
    """The price table with the region's VAT added to every price."""
    for sku, cents in prices.items():
        prices[sku] = add_vat(cents, region)
    return prices
