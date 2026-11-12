"""Stock inventory: parse the CSV export and report on it."""
import csv


def load(path):
    """Read the export into a list of {"sku", "name", "qty", "price"} dicts."""
    items = []
    with open(path, newline="") as f:
        for row in csv.DictReader(f):
            items.append({
                "sku": row["sku"],
                "name": row["name"],
                "qty": int(row["qty"]),
                "price": float(row["price"]),
            })
    return items


def stock_value(items):
    """Total value of everything in stock."""
    return sum(item["price"] * item["qty"] for item in items)


def low_stock(items, threshold=5):
    """SKUs whose quantity is at or below the threshold, sorted."""
    return sorted(item["sku"] for item in items if item["qty"] <= threshold)
