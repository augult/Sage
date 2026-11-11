import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import conf
from conf import ConfError

HERE = os.path.dirname(os.path.abspath(__file__))


class ConfTest(unittest.TestCase):
    def test_sections_and_entries(self):
        text = "top = 1\n\n# a comment\n[db]\nhost = localhost\nport=5432\n"
        self.assertEqual(conf.parse(text), {"": {"top": "1"}, "db": {"host": "localhost", "port": "5432"}})

    def test_inline_comment(self):
        self.assertEqual(conf.parse("path = /srv/app   # where it lives\n"), {"": {"path": "/srv/app"}})

    def test_quoted_value(self):
        self.assertEqual(conf.parse('greeting = "  hello # world  "   ; kept\n'), {"": {"greeting": "  hello # world  "}})

    def test_list(self):
        self.assertEqual(conf.parse("[s]\nx[] = 1\nx[] = 2\n"), {"s": {"x": ["1", "2"]}})

    def test_reference(self):
        self.assertEqual(conf.parse("base = /srv\n[app]\ndata = ${base}/data\n")["app"], {"data": "/srv/data"})

    def test_line_without_equals(self):
        with self.assertRaises(ConfError) as caught:
            conf.parse("a = 1\nnonsense\n")
        self.assertEqual(caught.exception.line, 2)

    def test_example_file(self):
        settings = conf.load(os.path.join(HERE, "..", "examples", "app.conf"))
        self.assertEqual(settings["server"]["listen"], ["127.0.0.1:8080", "[::1]:8080"])
        self.assertEqual(settings["paths"]["logs"], "/srv/reports/data/logs")


if __name__ == "__main__":
    unittest.main()
