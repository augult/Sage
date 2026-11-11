"""List prices per region, from the catalogue."""

import json
from pathlib import Path

_CATALOGUE = Path(__file__).with_name("catalogue.json")
_tables = {}


def catalogue():
    return json.loads(_CATALOGUE.read_text(encoding="utf-8"))


def vat_pct(region):
    return catalogue()["regions"][region]["vat_pct"]


def _build(region):
    data = catalogue()
    factor = data["regions"][region]["factor_pct"]
    return {sku: (cents * factor + 50) // 100 for sku, cents in data["base_cents"].items()}


def price_table(region):
    """SKU -> list price in cents for the region. An unknown region raises KeyError.

    Building a table reads the catalogue, so each region's table is built once and kept. The caller
    gets a copy: several callers rework the table they are given.
    """
    if region not in _tables:
        _tables[region] = _build(region)
    return dict(_tables[region])
