"""No call may change what a later call returns: every path that works on a price table."""

import unittest

from catalog import listing
from checkout import total
from invoice import invoice

UK = [("CASE", 3060), ("INK", 1161), ("PAD", 432), ("PEN", 225)]
CH = [("CASE", 4250), ("INK", 1613), ("PAD", 600), ("PEN", 313)]
CH_GROSS = [("CASE", 4590), ("INK", 1742), ("PAD", 648), ("PEN", 338)]


class StateTest(unittest.TestCase):
    def test_a_promotion_does_not_stick(self):
        self.assertEqual(total("US", {"INK": 2}, "INK25"), 2 * 1064)
        self.assertEqual(total("US", {"INK": 2}), 2 * 1419)
        self.assertEqual(invoice("US", {"INK": 1}), {"net": 1419, "vat": 0, "gross": 1419})

    def test_a_promotion_does_not_compound(self):
        first = total("EU", {"PEN": 4, "PAD": 1}, "SPRING10")
        self.assertEqual(first, 4 * 225 + 432)
        self.assertEqual(total("EU", {"PEN": 4, "PAD": 1}, "SPRING10"), first)
        self.assertEqual(total("EU", {"PEN": 4, "PAD": 1}), 4 * 250 + 480)

    def test_bundles_do_not_stay_in_the_list(self):
        self.assertEqual(len(listing("UK", bundles=True)), 6)
        self.assertEqual(listing("UK"), UK)
        self.assertEqual(listing("UK", gross=True), [("CASE", 3672), ("INK", 1393), ("PAD", 518), ("PEN", 270)])

    def test_vat_is_added_once(self):
        self.assertEqual(listing("CH", gross=True), CH_GROSS)
        self.assertEqual(listing("CH", gross=True), CH_GROSS)
        self.assertEqual(listing("CH"), CH)
        self.assertEqual(invoice("CH", {"PEN": 1}), {"net": 313, "vat": 25, "gross": 338})
        self.assertEqual(total("CH", {"PAD": 2}), 1200)

    def test_bundles_with_vat_then_plain(self):
        both = listing("CH", bundles=True, gross=True)
        self.assertEqual(dict(both)["KIT-STARTER"], 936)
        self.assertEqual(listing("CH", bundles=True, gross=True), both)
        self.assertEqual(listing("CH"), CH)


if __name__ == "__main__":
    unittest.main()
