"""Billable weight (README rules 3 and 4)."""


def volumetric_g(item):
    """The volumetric weight of the whole line, in grams."""
    length, width, height = item["dims_cm"]
    volume = length * width * height
    return -(-volume // 5) * item["qty"]


def billable_g(items):
    """The billable weight of the shipment, in grams."""
    actual = sum(item["qty"] * item["weight_g"] for item in items)
    volumetric = sum(volumetric_g(item) for item in items)
    return -(-max(actual, volumetric) // 500) * 500
