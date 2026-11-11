import unittest

from pager import page, total_pages


class PagerTest(unittest.TestCase):
    def test_total_pages_rounds_up(self):
        self.assertEqual(total_pages(10, 5), 2)
        self.assertEqual(total_pages(11, 5), 3)

    def test_first_page(self):
        self.assertEqual(page(list(range(1, 12)), 1, 5), [1, 2, 3, 4, 5])

    def test_last_page_is_partial(self):
        self.assertEqual(page(list(range(1, 12)), 3, 5), [11])


if __name__ == "__main__":
    unittest.main()
