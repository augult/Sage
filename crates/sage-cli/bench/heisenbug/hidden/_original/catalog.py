"""The price list shown in the shop."""

import tax
from bundles import with_bundles
from pricing import price_table


def listing(region, bundles=False, gross=False):
    """[(sku, cents)] in SKU order: the region's list prices, with bundles and VAT on request."""
    prices = price_table(region)
    if bundles:
        prices = with_bundles(prices)
    if gross:
        prices = tax.gross(prices, region)
    return sorted(prices.items())
