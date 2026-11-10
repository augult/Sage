import unittest

from textstats import top_words


class TopWordsMoreTest(unittest.TestCase):
    def test_fewer_distinct_words_than_n(self):
        self.assertEqual(top_words("x y x", 5), [("x", 2), ("y", 1)])

    def test_n_below_one_is_rejected(self):
        for n in (0, -2):
            with self.assertRaises(ValueError):
                top_words("a b", n)

    def test_case_is_ignored_when_counting(self):
        self.assertEqual(top_words("Tea tea TEA coffee", 1), [("tea", 3)])

    def test_empty_text(self):
        self.assertEqual(top_words("", 3), [])

    def test_ties_after_the_leader(self):
        self.assertEqual(top_words("z z y x w", 3), [("z", 2), ("w", 1), ("x", 1)])


if __name__ == "__main__":
    unittest.main()
