"""Surcharges (README rules 9 to 11)."""

from . import rates


def surcharges(items, base, remote):
    """The surcharges of a shipment as a dict; a surcharge of zero is left out."""
    found = {}
    if remote:
        found["remote"] = (base * rates.REMOTE_PERCENT + 50) // 100
    fragile_units = sum(item["qty"] for item in items if item.get("fragile"))
    found["fragile"] = min(fragile_units * rates.FRAGILE_PER_UNIT, rates.FRAGILE_CAP)
    if any(max(item["dims_cm"]) > rates.OVERSIZE_SIDE_CM or item["weight_g"] > rates.OVERSIZE_WEIGHT_G for item in items):
        found["oversize"] = rates.OVERSIZE_FEE
    return {name: cents for name, cents in found.items() if cents}
