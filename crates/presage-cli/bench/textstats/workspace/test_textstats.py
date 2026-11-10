import unittest

from textstats import top_words, word_count, words


class TextStatsTest(unittest.TestCase):
    def test_words_are_lower_cased(self):
        self.assertEqual(words("The cat. THE dog!"), ["the", "cat", "the", "dog"])

    def test_word_count(self):
        self.assertEqual(word_count("one two  three"), 3)

    def test_top_words_orders_by_count(self):
        self.assertEqual(top_words("b a b c b a", 2), [("b", 3), ("a", 2)])

    def test_top_words_breaks_ties_alphabetically(self):
        self.assertEqual(top_words("pear apple fig pear apple fig kiwi", 3), [("apple", 2), ("fig", 2), ("pear", 2)])

    def test_top_words_defaults_to_three(self):
        self.assertEqual(len(top_words("a b c d e")), 3)


if __name__ == "__main__":
    unittest.main()
