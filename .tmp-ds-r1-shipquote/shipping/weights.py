"""Billable weight (README rules 3 and 4)."""


def volumetric_g(item):
    """The volumetric weight of the whole line, in grams."""
    volume = 1
    for side in item["dims_cm"]:
        volume *= side
    return ((volume + 4) // 5) * item["qty"]


def billable_g(items):
    """The billable weight of the shipment, in grams."""
    actual = sum(item["weight_g"] * item["qty"] for item in items)
    volumetric = sum(volumetric_g(item) for item in items)
    grams = max(actual, volumetric)
    return ((grams + 499) // 500) * 500
