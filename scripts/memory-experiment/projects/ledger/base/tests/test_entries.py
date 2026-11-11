import unittest

from ledger.entries import Entry, parse_line, parse_lines


class ParseLine(unittest.TestCase):
    def test_four_fields(self):
        self.assertEqual(parse_line("2024-03-01,Food,-12.50,Bakery\n"), Entry("2024-03-01", "food", -1250, "Bakery"))

    def test_description_is_optional(self):
        self.assertEqual(parse_line("2024-03-02, rent , -800"), Entry("2024-03-02", "rent", -80000, ""))

    def test_quoted_description_keeps_its_comma(self):
        entry = parse_line('2024-03-03,food,-7.20,"Bread, milk"')
        self.assertEqual(entry.description, "Bread, milk")

    def test_too_few_fields(self):
        with self.assertRaises(ValueError):
            parse_line("2024-03-01,food")


class ParseLines(unittest.TestCase):
    def test_skips_blanks_and_comments(self):
        lines = ["# march\n", "\n", "2024-03-01,salary,2500\n", "2024-03-02,rent,-800\n"]
        self.assertEqual([e.category for e in parse_lines(lines)], ["salary", "rent"])


if __name__ == "__main__":
    unittest.main()
