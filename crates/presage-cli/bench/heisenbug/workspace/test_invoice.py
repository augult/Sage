import unittest

from invoice import invoice


class InvoiceTest(unittest.TestCase):
    def test_eu_invoice(self):
        self.assertEqual(invoice("EU", {"PEN": 4, "INK": 1}), {"net": 2290, "vat": 458, "gross": 2748})

    def test_us_invoice_has_no_vat(self):
        self.assertEqual(invoice("US", {"PAD": 3}), {"net": 1584, "vat": 0, "gross": 1584})

    def test_uk_invoice(self):
        self.assertEqual(invoice("UK", {"PAD": 2, "CASE": 1}), {"net": 3924, "vat": 785, "gross": 4709})


if __name__ == "__main__":
    unittest.main()
