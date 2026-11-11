"""The quote for an order (README rules 5 to 8 and 12)."""

from . import rates, surcharges, weights, zones
from .errors import QuoteError


def _positive_int(value):
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def _validate(order):
    items = order.get("items")
    if not items:
        raise QuoteError("an order needs at least one item")
    if order.get("service") not in ("standard", "express"):
        raise QuoteError(f"unknown service: {order.get('service')!r}")
    for item in items:
        if not _positive_int(item.get("qty")):
            raise QuoteError("qty must be a positive integer")
        if not _positive_int(item.get("weight_g")):
            raise QuoteError("weight_g must be a positive integer")
        dims = item.get("dims_cm")
        if not isinstance(dims, (list, tuple)) or len(dims) != 3 or not all(_positive_int(side) for side in dims):
            raise QuoteError("dims_cm must be three positive integers")


def quote(order):
    """The quote for `order`, as described in the README."""
    _validate(order)
    items = order["items"]
    express = order["service"] == "express"
    zone, remote = zones.lookup(order["destination"])
    if express and zone in rates.EXPRESS_UNAVAILABLE_IN:
        raise QuoteError(f"express is not available in zone {zone}")

    billable = weights.billable_g(items)
    tariff = rates.ZONES[zone]
    base = tariff["first_500"] + tariff["per_extra_500"] * (billable // 500 - 1)
    if express:
        base = (base * 3 + 1) // 2

    merchandise = sum(item["qty"] * item["price"] for item in items)
    if not express and not remote and merchandise >= rates.FREE_SHIPPING_FROM:
        base = 0

    coupon = order.get("coupon")
    if coupon == "SHIP50":
        base //= 2
    elif coupon == "FREESHIP":
        if not remote:
            base = 0
    elif coupon is not None:
        raise QuoteError(f"unknown coupon: {coupon!r}")

    extra = surcharges.surcharges(items, base, remote)
    return {"zone": zone, "billable_g": billable, "base": base, "surcharges": extra, "total": base + sum(extra.values())}
