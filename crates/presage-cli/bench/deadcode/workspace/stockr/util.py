"""Small helpers."""


def normalize_sku(sku):
    """SKUs compare without case or surrounding space: " a-100 " is "A-100"."""
    return sku.strip().upper()


def chunked(sequence, size):
    """Yields the sequence in lists of at most `size` items."""
    for start in range(0, len(sequence), size):
        yield sequence[start : start + size]


def money(value):
    return f"{value:,.2f}"
