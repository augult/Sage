"""Small helpers."""


def normalize_sku(sku):
    """SKUs compare without case or surrounding space: " a-100 " is "A-100"."""
    return sku.strip().upper()


def money(value):
    return f"{value:,.2f}"
