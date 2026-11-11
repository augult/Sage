import unittest

from ledger.entries import Entry
from ledger.report import balance, totals_by_category

ENTRIES = [
    Entry("2024-03-01", "salary", 250000),
    Entry("2024-03-02", "rent", -80000),
    Entry("2024-03-03", "food", -1250),
    Entry("2024-03-09", "food", -2075),
]


class Report(unittest.TestCase):
    def test_totals_by_category(self):
        self.assertEqual(totals_by_category(ENTRIES), {"salary": 250000, "rent": -80000, "food": -3325})

    def test_balance(self):
        self.assertEqual(balance(ENTRIES), 166675)

    def test_empty(self):
        self.assertEqual(totals_by_category([]), {})
        self.assertEqual(balance([]), 0)


if __name__ == "__main__":
    unittest.main()
