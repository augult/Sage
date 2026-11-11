import unittest

from ledger.entries import Entry, parse_line, parse_lines


class Quoting(unittest.TestCase):
    def test_comma_in_quotes(self):
        self.assertEqual(
            parse_line('2024-04-01,food,-3.10,"Tea, sugar, lemons"\n'),
            Entry("2024-04-01", "food", -310, "Tea, sugar, lemons"),
        )

    def test_doubled_quotes(self):
        self.assertEqual(parse_line('2024-04-02,gifts,-25,"A ""surprise"", wrapped"').description, 'A "surprise", wrapped')

    def test_unquoted_still_works(self):
        self.assertEqual(parse_line("2024-04-03, Rent ,-800,April"), Entry("2024-04-03", "rent", -80000, "April"))
        self.assertEqual(parse_line("2024-04-03,rent,-800").description, "")

    def test_lines(self):
        lines = ["# april\n", '2024-04-01,food,-3.10,"Tea, sugar"\n', "\n", "2024-04-05,salary,2500\n"]
        self.assertEqual([(e.category, e.description) for e in parse_lines(lines)], [("food", "Tea, sugar"), ("salary", "")])
