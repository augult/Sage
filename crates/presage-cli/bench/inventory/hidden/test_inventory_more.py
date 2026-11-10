import unittest

from inventory import low_stock, stock_value


def item(sku, qty, price):
    return {"sku": sku, "name": sku.lower(), "qty": qty, "price": price}


class InventoryMoreTest(unittest.TestCase):
    def test_empty_inventory_is_worth_nothing(self):
        self.assertEqual(stock_value([]), 0)

    def test_value_counts_every_unit(self):
        self.assertAlmostEqual(stock_value([item("X", 3, 1.5), item("Y", 2, 10.0)]), 24.5)

    def test_custom_threshold_is_inclusive(self):
        items = [item("B", 2, 1.0), item("A", 1, 1.0), item("C", 3, 1.0)]
        self.assertEqual(low_stock(items, threshold=2), ["A", "B"])


if __name__ == "__main__":
    unittest.main()
