import unittest
from pathlib import Path

from stockr import Inventory, Item

SAMPLE = Path(__file__).resolve().parent.parent / "sample.csv"


class InventoryTest(unittest.TestCase):
    def test_loads_the_sample(self):
        inventory = Inventory.load(SAMPLE)
        self.assertEqual(len(inventory), 5)
        self.assertEqual([item.sku for item in inventory], ["A-100", "B-220", "C-310", "D-404", "E-512"])
        self.assertIsNone(inventory.get("c-310").price)

    def test_skus_are_normalized(self):
        inventory = Inventory([Item(" a-1 ", "Awl", 3, 2.0)])
        self.assertEqual(inventory.get("A-1").name, "Awl")
        inventory.add(Item("A-1", "Awl, long", 5, 2.5))
        self.assertEqual(len(inventory), 1)
        self.assertEqual(inventory.remove("a-1").qty, 5)
        self.assertEqual(len(inventory), 0)

    def test_total_value_skips_missing_prices_and_negative_stock(self):
        inventory = Inventory.load(SAMPLE)
        self.assertAlmostEqual(inventory.total_value(), 4 * 129.50 + 60 * 1.20)

    def test_unknown_extension(self):
        with self.assertRaises(ValueError):
            Inventory.load("stock.xlsx")


if __name__ == "__main__":
    unittest.main()
