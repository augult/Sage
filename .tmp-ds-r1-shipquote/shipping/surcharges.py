"""Surcharges (README rules 9 to 11)."""

from . import rates


def surcharges(items, base, remote):
    """The surcharges of a shipment as a dict; a surcharge of zero is left out."""
    result = {}
    if remote:
        remote_fee = (base * rates.REMOTE_PERCENT + 50) // 100
        if remote_fee:
            result["remote"] = remote_fee
    fragile_units = sum(item["qty"] for item in items if item.get("fragile"))
    fragile = min(fragile_units * rates.FRAGILE_PER_UNIT, rates.FRAGILE_CAP)
    if fragile:
        result["fragile"] = fragile
    oversize = any(
        any(side > rates.OVERSIZE_SIDE_CM for side in item["dims_cm"])
        or item["weight_g"] > rates.OVERSIZE_WEIGHT_G
        for item in items
    )
    if oversize:
        result["oversize"] = rates.OVERSIZE_FEE
    return result
