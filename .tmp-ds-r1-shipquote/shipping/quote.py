"""The quote for an order (README rules 5 to 8 and 12)."""

from . import rates, surcharges, weights, zones
from .errors import QuoteError


def _is_positive_int(value):
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def _validate_items(items):
    for item in items:
        if not _is_positive_int(item.get("qty")):
            raise QuoteError("qty must be a positive integer")
        if not _is_positive_int(item.get("weight_g")):
            raise QuoteError("weight_g must be a positive integer")
        dims = item.get("dims_cm")
        if not isinstance(dims, (list, tuple)) or not dims:
            raise QuoteError("dims_cm must list the sides")
        if not all(_is_positive_int(side) for side in dims):
            raise QuoteError("every side must be a positive integer")


def quote(order):
    """The quote for `order`, as described in the README."""
    items = order.get("items")
    if not items:
        raise QuoteError("an order needs at least one item")
    _validate_items(items)

    service = order.get("service")
    if service not in ("standard", "express"):
        raise QuoteError(f"unknown service {service!r}")

    zone, remote = zones.lookup(order.get("destination"))
    if service == "express" and zone in rates.EXPRESS_UNAVAILABLE_IN:
        raise QuoteError(f"express is not available in zone {zone}")

    billable = weights.billable_g(items)
    tariff = rates.ZONES[zone]
    base = tariff["first_500"] + max(0, billable // 500 - 1) * tariff["per_extra_500"]
    if service == "express":
        base = (base * 3 + 1) // 2

    merchandise = sum(item["qty"] * item["price"] for item in items)
    if service == "standard" and not remote and merchandise >= rates.FREE_SHIPPING_FROM:
        base = 0

    coupon = order.get("coupon")
    if coupon is not None:
        if coupon == "SHIP50":
            base //= 2
        elif coupon == "FREESHIP":
            if not remote:
                base = 0
        else:
            raise QuoteError(f"unknown coupon {coupon!r}")

    fees = surcharges.surcharges(items, base, remote)
    return {
        "zone": zone,
        "billable_g": billable,
        "base": base,
        "surcharges": fees,
        "total": base + sum(fees.values()),
    }
