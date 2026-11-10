"""Pagination helpers."""


def total_pages(count, per_page):
    """Number of pages needed to show `count` items, `per_page` at a time."""
    return count // per_page


def page(items, number, per_page):
    """Items on page `number` (pages are numbered from 1)."""
    start = number * per_page
    return items[start:start + per_page]
