import unittest

from slug import slugify


class SlugMoreTest(unittest.TestCase):
    def test_underscores_separate_words(self):
        self.assertEqual(slugify("snake_case name"), "snake-case-name")

    def test_nothing_to_keep(self):
        self.assertEqual(slugify(""), "")
        self.assertEqual(slugify("?!  --"), "")

    def test_existing_dashes_do_not_double(self):
        self.assertEqual(slugify("well-known -- fact"), "well-known-fact")

    def test_other_scripts_are_letters_too(self):
        self.assertEqual(slugify("Привет, мир"), "привет-мир")


if __name__ == "__main__":
    unittest.main()
