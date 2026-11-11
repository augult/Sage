"""Billable weight (README rules 3 and 4)."""


def volumetric_g(item):
    """The volumetric weight of the whole line, in grams."""
    raise NotImplementedError


def billable_g(items):
    """The billable weight of the shipment, in grams."""
    raise NotImplementedError
