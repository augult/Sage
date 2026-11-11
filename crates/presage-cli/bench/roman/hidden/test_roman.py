import unittest

from roman import from_roman, to_roman


class RomanTest(unittest.TestCase):
    def test_known_values(self):
        known = [(1, "I"), (4, "IV"), (9, "IX"), (14, "XIV"), (40, "XL"), (90, "XC"), (400, "CD"), (1994, "MCMXCIV"), (2024, "MMXXIV"), (3999, "MMMCMXCIX")]
        for number, numeral in known:
            self.assertEqual(to_roman(number), numeral)
            self.assertEqual(from_roman(numeral), number)

    def test_round_trip(self):
        for number in range(1, 4000):
            self.assertEqual(from_roman(to_roman(number)), number)

    def test_numbers_out_of_range(self):
        for number in (0, -1, 4000):
            with self.assertRaises(ValueError, msg=str(number)):
                to_roman(number)

    def test_malformed_numerals(self):
        for numeral in ("", "IIII", "VX", "IC", "VV", "IXI", "MMMM", "ABC", "XM"):
            with self.assertRaises(ValueError, msg=repr(numeral)):
                from_roman(numeral)


if __name__ == "__main__":
    unittest.main()
