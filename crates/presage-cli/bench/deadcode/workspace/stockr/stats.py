"""Statistics over quantities."""

import math
from typing import Optional


def mean(values):
    values = list(values)
    return sum(values) / len(values) if values else 0.0


def median(values):
    ordered = sorted(values)
    if not ordered:
        return 0.0
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return float(ordered[middle])
    return (ordered[middle - 1] + ordered[middle]) / 2


def _old_median(values):
    ordered = sorted(values)
    return ordered[len(ordered) // 2] if ordered else 0


def percentile(values, p):
    """The nearest-rank percentile: the smallest value with at least p percent at or below it."""
    ordered = sorted(values)
    if not ordered:
        return 0.0
    rank = max(1, math.ceil(p / 100 * len(ordered)))
    return float(ordered[min(rank, len(ordered)) - 1])
