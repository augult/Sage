"""Invoices are always at list price: promotions are settled separately, as credit notes."""

from pricing import price_table, vat_pct


def invoice(region, cart):
    """{"net", "vat", "gross"} in cents for a cart {sku: quantity}. VAT is rounded to a cent, halves up."""
    prices = price_table(region)
    net = sum(prices[sku] * quantity for sku, quantity in cart.items())
    vat = (net * vat_pct(region) + 50) // 100
    return {"net": net, "vat": vat, "gross": net + vat}
