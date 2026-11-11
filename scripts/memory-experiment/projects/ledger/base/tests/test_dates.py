import unittest

from ledger.dates import month_key, month_range


class Dates(unittest.TestCase):
    def test_month_key(self):
        self.assertEqual(month_key("2024-03-17"), "2024-03")
        self.assertEqual(month_key("2024-3-1"), "2024-03")

    def test_month_range(self):
        self.assertEqual(month_range("2024-01", "2024-03"), ["2024-01", "2024-02", "2024-03"])

    def test_single_month(self):
        self.assertEqual(month_range("2024-06", "2024-06"), ["2024-06"])


if __name__ == "__main__":
    unittest.main()
