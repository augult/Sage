THOUGHT = (
    "This harness has write/edit, not apply_patch. Write the four modules in full "
    "and rerun the tests."
)
CONTENT = "This harness exposes `write` rather than `apply_patch`; writing the modules in full."

ZONES = r'''"""Postal code to zone (README rules 1 and 2)."""

from . import rates
from .errors import QuoteError


def normalise(postal_code):
    """The postal code without spaces, in upper case."""
    return str(postal_code).replace(" ", "").upper()


def lookup(postal_code):
    """`(zone, remote)` for a postal code as the customer typed it."""
    code = normalise(postal_code)
    best = None
    for prefix, zone, remote in rates.PREFIXES:
        if code.startswith(prefix) and (best is None or len(prefix) > best[0]):
            best = (len(prefix), zone, remote)
    if best is None:
        raise QuoteError(f"unknown postal code: {postal_code!r}")
    return best[1], best[2]
'''

WEIGHTS = r'''"""Billable weight (README rules 3 and 4)."""


def volumetric_g(item):
    """The volumetric weight of the whole line, in grams."""
    volume = 1
    for side in item["dims_cm"]:
        volume *= side
    return (volume + 4) // 5 * item["qty"]


def billable_g(items):
    """The billable weight of the shipment, in grams."""
    actual = sum(item["weight_g"] * item["qty"] for item in items)
    volumetric = sum(volumetric_g(item) for item in items)
    heaviest = max(actual, volumetric)
    return (heaviest + 499) // 500 * 500
'''

SURCHARGES = r'''"""Surcharges (README rules 9 to 11)."""

from . import rates


def surcharges(items, base, remote):
    """The surcharges of a shipment as a dict; a surcharge of zero is left out."""
    out = {}
    if remote:
        out["remote"] = (base * rates.REMOTE_PERCENT * 2 + 100) // 200
    units = sum(item["qty"] for item in items if item.get("fragile"))
    if units:
        out["fragile"] = min(units * rates.FRAGILE_PER_UNIT, rates.FRAGILE_CAP)
    oversized = any(
        max(item["dims_cm"]) > rates.OVERSIZE_SIDE_CM
        or item["weight_g"] > rates.OVERSIZE_WEIGHT_G
        for item in items
    )
    if oversized:
        out["oversize"] = rates.OVERSIZE_FEE
    return out
'''

QUOTE = r'''"""The quote for an order (README rules 5 to 8 and 12)."""

from . import rates, surcharges, weights, zones
from .errors import QuoteError


def quote(order):
    """The quote for `order`, as described in the README."""
    items = order.get("items")
    if not items:
        raise QuoteError("an order needs at least one item")
    service = order.get("service")
    if service not in ("standard", "express"):
        raise QuoteError(f"unknown service: {service!r}")
    for item in items:
        if not _positive_int(item.get("qty")):
            raise QuoteError("qty must be a positive integer")
        if not _positive_int(item.get("weight_g")):
            raise QuoteError("weight_g must be a positive integer")
        sides = item.get("dims_cm") or []
        if not sides or not all(_positive_int(side) for side in sides):
            raise QuoteError("dims_cm must be positive integers")

    zone, remote = zones.lookup(order.get("destination"))
    if service == "express" and zone in rates.EXPRESS_UNAVAILABLE_IN:
        raise QuoteError(f"express is not available in zone {zone}")

    billable = weights.billable_g(items)
    tariff = rates.ZONES[zone]
    base = tariff["first_500"] + max(0, billable - 500) // 500 * tariff["per_extra_500"]
    if service == "express":
        base = (base * 3 + 1) // 2

    merchandise = sum(item["price"] * item["qty"] for item in items)
    if service == "standard" and merchandise >= rates.FREE_SHIPPING_FROM and not remote:
        base = 0

    coupon = order.get("coupon")
    if coupon is not None:
        if coupon == "SHIP50":
            base //= 2
        elif coupon == "FREESHIP":
            if not remote:
                base = 0
        else:
            raise QuoteError(f"unknown coupon: {coupon!r}")

    extra = surcharges.surcharges(items, base, remote)
    return {
        "zone": zone,
        "billable_g": billable,
        "base": base,
        "surcharges": extra,
        "total": base + sum(extra.values()),
    }


def _positive_int(value):
    return isinstance(value, int) and not isinstance(value, bool) and value > 0
'''

CALLS = [
    {"name": "write", "arguments": {"filePath": "shipping/zones.py", "content": ZONES}},
    {"name": "write", "arguments": {"filePath": "shipping/weights.py", "content": WEIGHTS}},
    {"name": "write", "arguments": {"filePath": "shipping/surcharges.py", "content": SURCHARGES}},
    {"name": "write", "arguments": {"filePath": "shipping/quote.py", "content": QUOTE}},
    {"name": "bash", "arguments": {"command": "python3 -m unittest discover -s tests -v"}},
]
