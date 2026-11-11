"""The basket total at checkout."""

import promo
from pricing import price_table


def total(region, cart, promo_code=None):
    """The net total in cents of a cart {sku: quantity}, with a promotion code if one is given."""
    prices = price_table(region)
    if promo_code:
        prices = promo.apply(prices, promo_code)
    return sum(prices[sku] * quantity for sku, quantity in cart.items())
