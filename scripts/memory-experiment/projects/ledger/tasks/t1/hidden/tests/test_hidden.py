import unittest

from ledger.money import format_cents


class Thousands(unittest.TestCase):
    def test_groups(self):
        self.assertEqual(format_cents(123456789, thousands=True), "1,234,567.89")
        self.assertEqual(format_cents(100000, thousands=True), "1,000.00")

    def test_small_amounts_are_unchanged(self):
        self.assertEqual(format_cents(99999, thousands=True), "999.99")
        self.assertEqual(format_cents(5, thousands=True), "0.05")

    def test_negative(self):
        self.assertEqual(format_cents(-123450, thousands=True), "-1,234.50")
        self.assertEqual(format_cents(-5, thousands=True), "-0.05")

    def test_default_is_ungrouped(self):
        self.assertEqual(format_cents(123456789), "1234567.89")
        self.assertEqual(format_cents(123456789, thousands=False), "1234567.89")
