import unittest

from ledger.entries import Entry
from ledger.report import top_categories

ENTRIES = [
    Entry("2024-03-01", "salary", 250000),
    Entry("2024-03-02", "rent", -80000),
    Entry("2024-03-03", "food", -1250),
    Entry("2024-03-09", "food", -2075),
    Entry("2024-03-10", "books", -3325),
    Entry("2024-03-11", "refunds", -500),
    Entry("2024-03-12", "refunds", 500),
    Entry("2024-03-13", "transport", -4500),
]


class TopCategories(unittest.TestCase):
    def test_biggest_first(self):
        self.assertEqual(top_categories(ENTRIES, 2), [("rent", -80000), ("transport", -4500)])

    def test_ties_go_by_name(self):
        self.assertEqual(top_categories(ENTRIES, 4), [("rent", -80000), ("transport", -4500), ("books", -3325), ("food", -3325)])

    def test_income_and_zero_totals_are_left_out(self):
        names = [name for name, _ in top_categories(ENTRIES, 10)]
        self.assertEqual(names, ["rent", "transport", "books", "food"])

    def test_edges(self):
        self.assertEqual(top_categories([], 3), [])
        self.assertEqual(top_categories(ENTRIES, 0), [])
