import unittest

from stockr import Inventory, Item, build_report, median


class ReportTest(unittest.TestCase):
    def setUp(self):
        self.inventory = Inventory(
            [Item("N-1", "Nails", 40, 0.05, 10), Item("S-2", "Screws", 80, 0.10, 10), Item("W-3", "Washers", 30, 0.02, 10)]
        )

    def test_text_report(self):
        self.assertEqual(
            build_report(self.inventory),
            "items: 3\nunits: 150\nvalue: 10.60\nmean qty: 50.0\nmedian qty: 40.0\n",
        )

    def test_unknown_format(self):
        with self.assertRaises(ValueError):
            build_report(self.inventory, "yaml")

    def test_median(self):
        self.assertEqual(median([3, 1, 2]), 2.0)
        self.assertEqual(median([4, 1, 2, 3]), 2.5)


if __name__ == "__main__":
    unittest.main()
