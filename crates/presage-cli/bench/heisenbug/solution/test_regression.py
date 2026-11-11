"""The price tables are kept between calls, so no caller may be handed one it can change."""

import unittest

from catalog import listing
from checkout import total
from invoice import invoice
from pricing import price_table


class SharedPriceTableTest(unittest.TestCase):
    def test_a_promotion_leaves_list_prices_alone(self):
        self.assertEqual(total("EU", {"PEN": 4}, "SPRING10"), 900)
        self.assertEqual(invoice("EU", {"PEN": 4, "INK": 1}), {"net": 2290, "vat": 458, "gross": 2748})
        self.assertEqual(total("EU", {"PEN": 4}, "SPRING10"), 900)

    def test_bundles_and_vat_leave_list_prices_alone(self):
        plain = listing("UK")
        listing("UK", bundles=True, gross=True)
        self.assertEqual(listing("UK"), plain)

    def test_a_table_belongs_to_its_caller(self):
        price_table("US")["PEN"] = 1
        self.assertEqual(price_table("US")["PEN"], 275)


if __name__ == "__main__":
    unittest.main()
