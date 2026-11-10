import unittest

from pager import page, total_pages


class PagerMoreTest(unittest.TestCase):
    def test_page_counts_at_the_edges(self):
        self.assertEqual(total_pages(0, 5), 0)
        self.assertEqual(total_pages(1, 5), 1)
        self.assertEqual(total_pages(5, 5), 1)

    def test_middle_page(self):
        self.assertEqual(page(list("abcdefg"), 2, 3), ["d", "e", "f"])

    def test_page_past_the_end_is_empty(self):
        self.assertEqual(page([1, 2, 3], 5, 2), [])


if __name__ == "__main__":
    unittest.main()
