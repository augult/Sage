"""Reads comma-separated inventory files: sku,name,qty,price,reorder_at with a header row."""

import csv

from ..inventory import Item


def read(path, delimiter=","):
    items = []
    with open(path, newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle, delimiter=delimiter):
            price = (row.get("price") or "").strip()
            items.append(
                Item(
                    sku=row["sku"],
                    name=row["name"],
                    qty=int(row["qty"]),
                    price=float(price) if price else None,
                    reorder_at=int(row.get("reorder_at") or 0),
                )
            )
    return items
