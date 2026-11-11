import unittest

from ledger.money import format_cents, parse_amount


class ParseAmount(unittest.TestCase):
    def test_plain(self):
        self.assertEqual(parse_amount("12.50"), 1250)
        self.assertEqual(parse_amount("7"), 700)
        self.assertEqual(parse_amount("0.5"), 50)

    def test_signs(self):
        self.assertEqual(parse_amount("-3"), -300)
        self.assertEqual(parse_amount("+4.25"), 425)

    def test_rejects_garbage(self):
        for bad in ("", "abc", "1.234", "1,50"):
            with self.assertRaises(ValueError):
                parse_amount(bad)


class FormatCents(unittest.TestCase):
    def test_plain(self):
        self.assertEqual(format_cents(1250), "12.50")
        self.assertEqual(format_cents(5), "0.05")
        self.assertEqual(format_cents(123456), "1234.56")

    def test_negative(self):
        self.assertEqual(format_cents(-300), "-3.00")


if __name__ == "__main__":
    unittest.main()
