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


def order(destination="10115", service="standard", items=None, **over):
    made = {"destination": destination, "service": service, "items": items if items is not None else [item()]}
    made.update(over)
    return made


class ZoneTest(unittest.TestCase):
    def test_rule_1_normalisation(self):
        self.assertEqual(lookup(" 1 01 15 "), ("A", False))
        self.assertEqual(lookup("ab1 2cd"), ("C", True))
        self.assertEqual(lookup("a1b"), ("B", False))

    def test_rule_2_longest_prefix(self):
        self.assertEqual(lookup("29000"), ("B", True))
        self.assertEqual(lookup("68410"), ("C", True))
        self.assertEqual(lookup("68100"), ("B", False))
        self.assertEqual(lookup("98123"), ("D", True))
        self.assertEqual(lookup("90000"), ("D", False))

    def test_rule_2_unknown(self):
        for code in ("00000", "", "Z1"):
            with self.assertRaises(QuoteError, msg=code):
                lookup(code)
        with self.assertRaises(QuoteError):
            quote(order(destination="00000"))


class WeightTest(unittest.TestCase):
    def test_rule_3_rounds_up_per_unit(self):
        got = quote(order(items=[item(qty=29, weight_g=10, dims_cm=[7, 7, 7])]))
        self.assertEqual(got["billable_g"], 2500)

    def test_rule_4_compares_shipment_totals(self):
        items = [item(weight_g=3000), item(weight_g=200, dims_cm=[50, 40, 30])]
        self.assertEqual(quote(order(items=items))["billable_g"], 12500)

    def test_rule_4_multiples_stay(self):
        self.assertEqual(quote(order(items=[item(weight_g=1000)]))["billable_g"], 1000)
        self.assertEqual(quote(order(items=[item(weight_g=1001)]))["billable_g"], 1500)

    def test_sides_in_any_order(self):
        self.assertEqual(quote(order(items=[item(dims_cm=[30, 50, 40])]))["billable_g"], 12000)


class BaseTest(unittest.TestCase):
    def test_rule_5_zone_tariffs(self):
        self.assertEqual(quote(order(destination="68100", items=[item(weight_g=1600)]))["base"], 695 + 3 * 165)
        self.assertEqual(quote(order(destination="90000", items=[item(weight_g=600)]))["base"], 1475 + 385)

    def test_rule_6_half_a_cent_goes_up(self):
        self.assertEqual(quote(order(service="express"))["base"], 743)
        self.assertEqual(quote(order(destination="60311", service="express"))["base"], 1388)

    def test_rule_6_no_express_in_zone_d(self):
        with self.assertRaises(QuoteError):
            quote(order(destination="90000", service="express"))
        with self.assertRaises(QuoteError):
            quote(order(destination="98123", service="express", coupon="FREESHIP"))

    def test_rule_7_free_shipping(self):
        self.assertEqual(quote(order(items=[item(qty=4, price=2500, weight_g=100)]))["base"], 0)
        self.assertEqual(quote(order(items=[item(qty=3, price=3333, weight_g=100, dims_cm=[5, 5, 5])]))["base"], 495)

    def test_rule_7_not_for_express_or_remote(self):
        rich = [item(qty=1, price=20000)]
        self.assertEqual(quote(order(service="express", items=rich))["base"], 743)
        self.assertEqual(quote(order(destination="29000", items=rich))["base"], 695)

    def test_rule_7_keeps_the_surcharges(self):
        got = quote(order(items=[item(price=15000, fragile=True)]))
        self.assertEqual((got["base"], got["surcharges"], got["total"]), (0, {"fragile": 250}, 250))

    def test_rule_8_ship50_rounds_down(self):
        self.assertEqual(quote(order(coupon="SHIP50"))["base"], 247)
        self.assertEqual(quote(order(service="express", coupon="SHIP50"))["base"], 371)

    def test_rule_8_freeship(self):
        self.assertEqual(quote(order(service="express", coupon="FREESHIP"))["total"], 0)
        self.assertEqual(quote(order(destination="29000", coupon="FREESHIP"))["base"], 695)

    def test_rule_8_other_codes(self):
        for code in ("ship50", "WELCOME", "Freeship"):
            with self.assertRaises(QuoteError, msg=code):
                quote(order(coupon=code))
        self.assertEqual(quote(order(coupon=None))["base"], 495)


class SurchargeTest(unittest.TestCase):
    def test_rule_9_remote(self):
        got = quote(order(destination="29000"))
        self.assertEqual(got, {"zone": "B", "billable_g": 500, "base": 695, "surcharges": {"remote": 104}, "total": 799})

    def test_rule_9_half_a_cent_goes_up(self):
        got = quote(order(destination="29000", items=[item(weight_g=2000)]))
        self.assertEqual((got["base"], got["surcharges"]), (1190, {"remote": 179}))
        got = quote(order(destination="AB1 2CD", items=[item(weight_g=1900)]))
        self.assertEqual((got["base"], got["surcharges"]), (1630, {"remote": 245}))

    def test_rule_9_is_on_the_base_after_coupon_and_express(self):
        got = quote(order(destination="29000", items=[item(weight_g=2000)], coupon="SHIP50"))
        self.assertEqual((got["base"], got["surcharges"], got["total"]), (595, {"remote": 89}, 684))
        got = quote(order(destination="68410", service="express"))
        self.assertEqual((got["base"], got["surcharges"], got["total"]), (1388, {"remote": 208}, 1596))

    def test_rule_10_fragile_units_and_cap(self):
        self.assertEqual(quote(order(items=[item(qty=3, weight_g=100, fragile=True)]))["surcharges"], {"fragile": 750})
        items = [item(qty=4, weight_g=100, fragile=True), item(qty=3, weight_g=100, fragile=True), item(qty=5, weight_g=100)]
        self.assertEqual(quote(order(items=items))["surcharges"], {"fragile": 1500})
        self.assertEqual(quote(order(items=[item(fragile=False)]))["surcharges"], {})

    def test_rule_11_oversize(self):
        self.assertEqual(quote(order(items=[item(dims_cm=[10, 121, 10])]))["surcharges"], {"oversize": 1200})
        self.assertEqual(quote(order(items=[item(dims_cm=[120, 10, 10])]))["surcharges"], {})
        self.assertEqual(quote(order(items=[item(weight_g=30001)]))["surcharges"], {"oversize": 1200})
        self.assertEqual(quote(order(items=[item(weight_g=30000)]))["surcharges"], {})

    def test_rule_11_once_and_per_unit(self):
        items = [item(dims_cm=[130, 10, 10]), item(weight_g=40000, qty=2)]
        self.assertEqual(quote(order(items=items))["surcharges"], {"oversize": 1200})
        self.assertEqual(quote(order(items=[item(weight_g=16000, qty=2)]))["surcharges"], {})

    def test_everything_at_once(self):
        items = [
            item(qty=2, price=3490, weight_g=1800, dims_cm=[40, 30, 25], fragile=True),
            item(qty=1, price=900, weight_g=250, dims_cm=[125, 8, 8]),
        ]
        got = quote(order(destination="68 410", service="express", items=items, coupon="SHIP50"))
        self.assertEqual(got["zone"], "C")
        self.assertEqual(got["billable_g"], 14000)
        self.assertEqual(got["base"], 5452)
        self.assertEqual(got["surcharges"], {"remote": 818, "fragile": 500, "oversize": 1200})
        self.assertEqual(got["total"], 7970)


class ValidationTest(unittest.TestCase):
    def test_rule_12(self):
        bad = [
            [],
            [item(qty=0)],
            [item(qty=-1)],
            [item(qty=1.5)],
            [item(qty="2")],
            [item(weight_g=0)],
            [item(weight_g=12.5)],
            [item(dims_cm=[10, 0, 10])],
            [item(dims_cm=[10, 2.5, 10])],
            [item(), item(qty=0)],
        ]
        for items in bad:
            with self.assertRaises(QuoteError, msg=repr(items)):
                quote(order(items=items))


if __name__ == "__main__":
    unittest.main()
