"""Summaries over a list of entries."""

from collections import defaultdict


def totals_by_category(entries) -> dict[str, int]:
    totals: dict[str, int] = defaultdict(int)
    for entry in entries:
        totals[entry.category] += entry.cents
    return dict(totals)


def balance(entries) -> int:
    return sum(entry.cents for entry in entries)
