import unittest

from weblog import analyze

LOG = [
    ("ana", 100, "/docs/"),
    ("ben", 110, "/"),
    ("ana", 160, "/docs/install?os=mac"),
    ("cy", 200, "/Pricing"),
    ("ben", 4000, "/docs"),
    ("ana", 300, "/docs/install#step-2"),
    ("ben", 4100, "/pricing/"),
    ("ana", 9000, "/"),
]


class AnalyzeTest(unittest.TestCase):
    def test_a_small_log(self):
        result = analyze(LOG)
        self.assertEqual(result["users"], ["ana", "ben", "cy"])
        self.assertEqual(result["sessions"], {"ana": 2, "ben": 2, "cy": 1})
        self.assertEqual(result["longest"], ("ana", 100, 300, ["/docs", "/docs/install"]))
        self.assertEqual(result["top_pages"], [("/docs", 2), ("/", 2), ("/pricing", 2), ("/docs/install", 1)])
        self.assertEqual(result["busiest"], "ana")

    def test_the_gap_is_inclusive(self):
        log = [("ana", 0, "/a"), ("ana", 60, "/b"), ("ana", 121, "/c")]
        self.assertEqual(analyze(log, gap=60)["sessions"], {"ana": 2})
        self.assertEqual(analyze(log, gap=61)["sessions"], {"ana": 1})

    def test_no_events(self):
        self.assertEqual(
            analyze([]), {"users": [], "sessions": {}, "longest": None, "top_pages": [], "busiest": None}
        )


if __name__ == "__main__":
    unittest.main()
