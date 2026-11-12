"""Items and the inventory that holds them."""

import importlib
import os
from dataclasses import dataclass
from pathlib import Path

from .util import chunked, normalize_sku


@dataclass
class Item:
    sku: str
    name: str
    qty: int
    price: float | None = None
    reorder_at: int = 0


class Inventory:
    def __init__(self, items=()):
        self._items = {}
        for item in items:
            self.add(item)

    @classmethod
    def load(cls, path):
        """Reads an inventory file. The reader is chosen by the file's extension."""
        extension = Path(path).suffix.lstrip(".").lower()
        try:
            reader = importlib.import_module(f"stockr.readers.{extension}_reader")
        except ModuleNotFoundError:
            raise ValueError(f"no reader for .{extension} files") from None
        return cls(reader.read(path))

    # def load_legacy(cls, path):
    #     items = []
    #     for line in open(path):
    #         sku, qty = line.split(";")
    #         items.append(Item(sku, sku, int(qty)))
    #     return cls(items)

    def add(self, item):
        """Adds an item. An item with the same SKU is replaced."""
        item.sku = normalize_sku(item.sku)
        self._items[item.sku] = item

    def remove(self, sku):
        return self._items.pop(normalize_sku(sku), None)

    def get(self, sku):
        return self._items.get(normalize_sku(sku))

    def __iter__(self):
        return iter(sorted(self._items.values(), key=lambda item: item.sku))

    def __len__(self):
        return len(self._items)

    def total_value(self):
        """The value of the stock on hand. Items without a price count as nothing."""
        return sum(item.qty * item.price for item in self if item.price is not None and item.qty > 0)

    def _reindex(self):
        fresh = {}
        for item in self._items.values():
            fresh[normalize_sku(item.sku)] = item
        self._items = fresh


def merge_duplicates(items):
    """Folds items with the same SKU into one, adding up the quantities."""
    merged = {}
    for batch in chunked(list(items), 50):
        for item in batch:
            key = normalize_sku(item.sku)
            if key in merged:
                merged[key].qty += item.qty
            else:
                merged[key] = item
    return list(merged.values())
