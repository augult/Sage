THOUGHT = (
    "Implement longest-prefix zone lookup, integer weight maths, surcharges and the "
    "quote pipeline, then run the visible tests."
)
CONTENT = "Implementing the four modules, then running the tests."

ZONES = r'''"""Postal code to zone (README rules 1 and 2)."""

from . import rates
from .errors import QuoteError


def normalise(postal_code):
    """The postal code without spaces, in upper case."""
    return str(postal_code).replace(" ", "").upper()


def lookup(postal_code):
    """`(zone, remote)` for a postal code as the customer typed it."""
    code = normalise(postal_code)
    matches = [
        (len(prefix), zone, remote)
        for prefix, zone, remote in rates.PREFIXES
        if code.startswith(prefix)
    ]
    if not matches:
        raise QuoteError(f"unknown postal code: {postal_code!r}")
    _, zone, remote = max(matches, key=lambda match: match[0])
    return zone, remote
'''

WEIGHTS = r'''"""Billable weight (README rules 3 and 4)."""


def volumetric_g(item):
    """The volumetric weight of the whole line, in grams."""
    volume = item["dims_cm"][0]
    for side in item["dims_cm"][1:]:
        volume *= side
    per_unit = (volume + 4) // 5
    return per_unit * item["qty"]


def billable_g(items):
    """The billable weight of the shipment, in grams."""
    actual = sum(item["weight_g"] * item["qty"] for item in items)
    volumetric = sum(volumetric_g(item) for item in items)
    heaviest = max(actual, volumetric)
    return ((heaviest + 499) // 500) * 500
'''

SURCHARGES = r'''"""Surcharges (README rules 9 to 11)."""

from . import rates


def surcharges(items, base, remote):
    """The surcharges of a shipment as a dict; a surcharge of zero is left out."""
    fees = {}
    if remote:
        # base * 15 / 100, half a cent rounded up.
        fees["remote"] = (base * rates.REMOTE_PERCENT * 2 + 100) // 200
    fragile_units = sum(item["qty"] for item in items if item.get("fragile"))
    if fragile_units:
        fees["fragile"] = min(
            fragile_units * rates.FRAGILE_PER_UNIT, rates.FRAGILE_CAP
        )
    if any(
        max(item["dims_cm"]) > rates.OVERSIZE_SIDE_CM
        or item["weight_g"] > rates.OVERSIZE_WEIGHT_G
        for item in items
    ):
        fees["oversize"] = rates.OVERSIZE_FEE
    return fees
'''

QUOTE = r'''"""The quote for an order (README rules 5 to 8 and 12)."""

from . import rates, surcharges, weights, zones
from .errors import QuoteError

SERVICES = ("standard", "express")


def _is_positive_int(value):
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def _validate(items, service):
    if not items:
        raise QuoteError("an order needs at least one item")
    if service not in SERVICES:
        raise QuoteError(f"unknown service: {service!r}")
    for item in items:
        if not _is_positive_int(item.get("qty")):
            raise QuoteError("qty must be a positive integer")
        if not _is_positive_int(item.get("weight_g")):
            raise QuoteError("weight_g must be a positive integer")
        sides = item.get("dims_cm") or []
        if not sides or not all(_is_positive_int(side) for side in sides):
            raise QuoteError("dims_cm must be positive integers")


def quote(order):
    """The quote for `order`, as described in the README."""
    items = order.get("items")
    service = order.get("service")
    _validate(items, service)

    zone, remote = zones.lookup(order.get("destination"))
    if service == "express" and zone in rates.EXPRESS_UNAVAILABLE_IN:
        raise QuoteError(f"express is not available in zone {zone}")

    billable = weights.billable_g(items)
    tariff = rates.ZONES[zone]
    extra_blocks = max(0, billable - 500) // 500
    base = tariff["first_500"] + extra_blocks * tariff["per_extra_500"]
    if service == "express":
        base = (base * 3 + 1) // 2  # 1.5x, half a cent up

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

    fees = surcharges.surcharges(items, base, remote)
    return {
        "zone": zone,
        "billable_g": billable,
        "base": base,
        "surcharges": fees,
        "total": base + sum(fees.values()),
    }
'''

CALLS = [
    {"name": "write", "arguments": {"filePath": "shipping/zones.py", "content": ZONES}},
    {"name": "write", "arguments": {"filePath": "shipping/weights.py", "content": WEIGHTS}},
    {"name": "write", "arguments": {"filePath": "shipping/surcharges.py", "content": SURCHARGES}},
    {"name": "write", "arguments": {"filePath": "shipping/quote.py", "content": QUOTE}},
    {"name": "bash", "arguments": {"command": "python3 -m unittest discover -s tests"}},
]
