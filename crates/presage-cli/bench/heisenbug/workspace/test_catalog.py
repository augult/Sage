import unittest

from catalog import listing


class CatalogTest(unittest.TestCase):
    def test_list_prices(self):
        self.assertEqual(listing("US"), [("CASE", 3740), ("INK", 1419), ("PAD", 528), ("PEN", 275)])

    def test_with_bundles(self):
        self.assertEqual(
            listing("UK", bundles=True),
            [("CASE", 3060), ("INK", 1161), ("KIT-PRO", 4634), ("KIT-STARTER", 624), ("PAD", 432), ("PEN", 225)],
        )

    def test_with_vat(self):
        self.assertEqual(listing("CH", gross=True), [("CASE", 4590), ("INK", 1742), ("PAD", 648), ("PEN", 338)])

    def test_unknown_region(self):
        with self.assertRaises(KeyError):
            listing("MARS")


if __name__ == "__main__":
    unittest.main()
