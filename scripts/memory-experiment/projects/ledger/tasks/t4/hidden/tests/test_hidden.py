import unittest

from ledger.dates import month_range


class MonthRange(unittest.TestCase):
    def test_across_a_year_boundary(self):
        self.assertEqual(month_range("2023-10", "2024-02"), ["2023-10", "2023-11", "2023-12", "2024-01", "2024-02"])

    def test_ends_in_december(self):
        self.assertEqual(month_range("2024-10", "2024-12"), ["2024-10", "2024-11", "2024-12"])

    def test_two_years(self):
        months = month_range("2022-01", "2024-01")
        self.assertEqual(len(months), 25)
        self.assertEqual(months[11:13], ["2022-12", "2023-01"])

    def test_single_and_empty(self):
        self.assertEqual(month_range("2024-12", "2024-12"), ["2024-12"])
        self.assertEqual(month_range("2024-05", "2024-03"), [])
