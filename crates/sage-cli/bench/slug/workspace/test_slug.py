import unittest

from slug import slugify


class SlugTest(unittest.TestCase):
    def test_simple(self):
        self.assertEqual(slugify("Hello World"), "hello-world")

    def test_punctuation_separates_words(self):
        self.assertEqual(slugify("Rock & Roll: live!"), "rock-roll-live")

    def test_repeated_spaces_collapse(self):
        self.assertEqual(slugify("  a   b  "), "a-b")

    def test_accents_are_kept(self):
        self.assertEqual(slugify("Café Münster"), "café-münster")

    def test_digits_are_kept(self):
        self.assertEqual(slugify("Top 10 of 2024"), "top-10-of-2024")


if __name__ == "__main__":
    unittest.main()
