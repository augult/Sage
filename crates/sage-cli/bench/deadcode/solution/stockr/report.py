"""Builds the stock report."""

from . import checks  # registers the checks
from . import formatters
from .registry import run_checks
from .stats import mean, median


def summarize(inventory):
    quantities = [item.qty for item in inventory]
    return {
        "items": len(inventory),
        "units": sum(quantity for quantity in quantities if quantity > 0),
        "value": round(inventory.total_value(), 2),
        "mean_qty": mean(quantities),
        "median_qty": median(quantities),
        "problems": [list(problem) for problem in run_checks(inventory)],
    }


def build_report(inventory, fmt="text"):
    """The report in the given format: text, csv or json."""
    formatter = getattr(formatters, f"format_{fmt}", None)
    if formatter is None:
        raise ValueError(f"unknown format: {fmt}")
    return formatter(summarize(inventory))
