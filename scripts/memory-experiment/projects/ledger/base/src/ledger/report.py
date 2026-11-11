"""Summaries over a list of entries."""

from collections import defaultdict


def totals_by_category(entries) -> dict[str, int]:
    totals: dict[str, int] = defaultdict(int)
    for entry in entries:
        totals[entry.category] += entry.cents
    return dict(totals)


def balance(entries) -> int:
    return sum(entry.cents for entry in entries)


def top_categories(entries, n: int) -> list[tuple[str, int]]:
    """The n categories with the largest spending (most negative total), biggest spender first.

    Categories whose total is zero or positive are not spending and are left out; ties go by name.
    """
    spending = [(category, total) for category, total in totals_by_category(entries).items() if total < 0]
    spending.sort(key=lambda pair: (pair[1], pair[0]))
    return spending[:n]
