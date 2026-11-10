import os
import unittest

from inventory import load, low_stock, stock_value

HERE = os.path.dirname(os.path.abspath(__file__))


class InventoryTest(unittest.TestCase):
    def setUp(self):
        self.items = load(os.path.join(HERE, "stock.csv"))

    def test_load_reads_every_row(self):
        self.assertEqual(len(self.items), 4)
        self.assertEqual(self.items[0], {"sku": "A-100", "name": "widget", "qty": 12, "price": 2.5})

    def test_stock_value_multiplies_by_quantity(self):
        self.assertAlmostEqual(stock_value(self.items), 12 * 2.5 + 5 * 10.0 + 0 * 99.0 + 3 * 4.0)

    def test_low_stock_includes_the_threshold(self):
        self.assertEqual(low_stock(self.items), ["B-200", "C-300", "D-400"])


if __name__ == "__main__":
    unittest.main()
