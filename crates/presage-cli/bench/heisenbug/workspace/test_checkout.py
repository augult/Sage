import unittest

from checkout import total


class CheckoutTest(unittest.TestCase):
    def test_plain_total(self):
        self.assertEqual(total("US", {"PEN": 2, "CASE": 1}), 2 * 275 + 3740)

    def test_promotion(self):
        self.assertEqual(total("EU", {"PEN": 4, "PAD": 1, "INK": 1}, "SPRING10"), 4 * 225 + 432 + 1290)

    def test_unknown_promotion(self):
        with self.assertRaises(ValueError):
            total("US", {"PEN": 1}, "WINTER99")


if __name__ == "__main__":
    unittest.main()
