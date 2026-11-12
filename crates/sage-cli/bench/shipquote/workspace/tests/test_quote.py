import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from shipping.errors import QuoteError
from shipping.quote import quote
from shipping.zones import lookup


def item(**over):
    base = {"sku": "X", "qty": 1, "price": 1000, "weight_g": 400, "dims_cm": [10, 10, 10]}
    base.update(over)
    return base


class QuoteTest(unittest.TestCase):
    def test_zone_of_a_plain_code(self):
        self.assertEqual(lookup("10115"), ("A", False))
        self.assertEqual(lookup("60311"), ("C", False))

    def test_smallest_parcel(self):
        got = quote({"destination": "10115", "service": "standard", "items": [item()]})
        self.assertEqual(got, {"zone": "A", "billable_g": 500, "base": 495, "surcharges": {}, "total": 495})

    def test_heavier_parcel(self):
        got = quote({"destination": "30159", "service": "standard", "items": [item(qty=3, weight_g=700)]})
        self.assertEqual(got["billable_g"], 2500)
        self.assertEqual(got["base"], 695 + 4 * 165)

    def test_express(self):
        got = quote({"destination": "10115", "service": "express", "items": [item(weight_g=900)]})
        self.assertEqual(got["base"], 908)

    def test_one_fragile_unit(self):
        got = quote({"destination": "10115", "service": "standard", "items": [item(fragile=True)]})
        self.assertEqual(got["surcharges"], {"fragile": 250})
        self.assertEqual(got["total"], 745)

    def test_unknown_service(self):
        with self.assertRaises(QuoteError):
            quote({"destination": "10115", "service": "overnight", "items": [item()]})


if __name__ == "__main__":
    unittest.main()
