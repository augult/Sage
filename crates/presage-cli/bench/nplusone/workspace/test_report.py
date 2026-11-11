import unittest

import shop
from report import customer_report


def database():
    conn = shop.connect()
    shop.create(conn)
    conn.executemany(
        "INSERT INTO customers (id, name, region) VALUES (?, ?, ?)",
        [(1, "Aster Ltd", "north"), (2, "Birch & Co", "south"), (3, "Cedar GmbH", "north")],
    )
    conn.executemany(
        "INSERT INTO orders (id, customer_id, placed_on, status, currency) VALUES (?, ?, ?, ?, ?)",
        [
            (10, 1, "2024-02-01", "paid", "EUR"),
            (11, 1, "2024-03-15", "shipped", "USD"),
            (12, 2, "2024-01-20", "cancelled", "EUR"),
            (13, 2, "2024-01-25", "open", "GBP"),
        ],
    )
    conn.executemany(
        "INSERT INTO items (order_id, sku, qty, unit_cents, discount_pct) VALUES (?, ?, ?, ?, ?)",
        [
            (10, "BOLT", 10, 250, None),
            (10, "NUT", 4, 100, 10),
            (11, "BOLT", 2, 300, None),
            (12, "GEAR", 50, 900, None),
            (13, "NUT", 3, 120, None),
        ],
    )
    return conn


class ReportTest(unittest.TestCase):
    def test_report(self):
        self.assertEqual(
            customer_report(database()),
            [
                {
                    "customer": "Aster Ltd",
                    "region": "north",
                    "orders": 2,
                    "total_cents": 2860 + 553,
                    "last_order": "2024-03-15",
                    "top_sku": "BOLT",
                },
                {
                    "customer": "Birch & Co",
                    "region": "south",
                    "orders": 1,
                    "total_cents": 421,
                    "last_order": "2024-01-25",
                    "top_sku": "NUT",
                },
                {
                    "customer": "Cedar GmbH",
                    "region": "north",
                    "orders": 0,
                    "total_cents": 0,
                    "last_order": None,
                    "top_sku": None,
                },
            ],
        )


if __name__ == "__main__":
    unittest.main()
