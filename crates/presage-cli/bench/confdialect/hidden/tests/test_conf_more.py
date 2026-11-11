import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import conf
from conf import ConfError


class Base(unittest.TestCase):
    def value(self, line, **context):
        """The value of `k` in a document that consists of `line`."""
        return conf.parse(line + "\n")[""]["k"]

    def fails(self, text, *lines):
        with self.assertRaises(ConfError, msg=repr(text)) as caught:
            conf.parse(text)
        self.assertIn(caught.exception.line, lines, repr(text))


class LinesAndSections(Base):
    def test_1_line_endings_and_blank_lines(self):
        self.assertEqual(conf.parse("a = 1\r\n \t \r\n[s]\r\nb = 2\r\n"), {"": {"a": "1"}, "s": {"b": "2"}})
        self.assertEqual(conf.parse("  ; c\n\t# c\n a = 1"), {"": {"a": "1"}})
        self.assertEqual(conf.parse(""), {})

    def test_2_headers(self):
        self.assertEqual(conf.parse("  [ db.primary ]  \nx = 1\n"), {"db.primary": {"x": "1"}})
        self.assertEqual(conf.parse("[A]\nx = 1\n[a]\nx = 2\n"), {"A": {"x": "1"}, "a": {"x": "2"}})

    def test_2_header_occurs_twice(self):
        got = conf.parse("[a]\nx = 1\n[b]\ny = 2\n[a]\nz = 3\n")
        self.assertEqual(got, {"a": {"x": "1", "z": "3"}, "b": {"y": "2"}})
        self.assertEqual(list(got), ["a", "b"])
        self.assertEqual(list(got["a"]), ["x", "z"])

    def test_2_bad_headers(self):
        self.fails("a = 1\n[]\n", 2)
        self.fails("[a b]\n", 1)
        self.fails("\n\n[a] # comment\n", 3)
        self.fails("[a\n", 1)
        self.fails("[a/b]\n", 1)

    def test_10_empty_and_top_sections(self):
        self.assertEqual(conf.parse("[empty]\n[s]\nx = 1\n"), {"empty": {}, "s": {"x": "1"}})
        self.assertNotIn("", conf.parse("[s]\nx = 1\n"))
        self.assertEqual(conf.parse("\ufeffa = 1\n"), {"": {"a": "1"}})


class Entries(Base):
    def test_3_first_equals_sign_splits(self):
        self.assertEqual(self.value("k = a=b = c"), "a=b = c")
        self.assertEqual(self.value("k="), "")
        self.assertEqual(self.value("\tk\t=\tv\t"), "v")
        self.assertEqual(conf.parse("a-b.c_9 = 1\n"), {"": {"a-b.c_9": "1"}})

    def test_3_bad_keys(self):
        self.fails("a = 1\n = 2\n", 2)
        self.fails("two words = 2\n", 1)
        self.fails("k! = 2\n", 1)
        self.fails("[] = 2\n", 1)

    def test_3_duplicate_key(self):
        self.fails("a = 1\nb = 2\na = 3\n", 3)
        self.fails("[s]\na = 1\n[t]\na = 1\n[s]\na = 2\n", 6)
        self.assertEqual(conf.parse("a = 1\n[s]\na = 2\n"), {"": {"a": "1"}, "s": {"a": "2"}})

    def test_3_no_equals(self):
        self.fails("a = 1\n\n# c\nnonsense here\n", 4)


class PlainValues(Base):
    def test_4_inline_comments(self):
        self.assertEqual(self.value("k = /srv/app \t; where"), "/srv/app")
        self.assertEqual(self.value("k = #ff8800"), "#ff8800")
        self.assertEqual(self.value("k = a#b ;c"), "a#b")
        self.assertEqual(self.value("k = # not a comment"), "# not a comment")
        self.assertEqual(self.value("k = ;x #y"), ";x")
        self.assertEqual(self.value("k = a;b#c"), "a;b#c")
        self.assertEqual(self.value("k = one two\t#three"), "one two")

    def test_5_continuation(self):
        self.assertEqual(self.value("k = alpha \\\n        beta \\\n  gamma   # three"), "alpha beta gamma")
        self.assertEqual(self.value("k = a\\\nb"), "a b")
        self.assertEqual(self.value("k = a \\  \t\n b"), "a b")

    def test_5_next_line_is_taken_as_it_is(self):
        self.assertEqual(conf.parse("k = a \\\n[s]\nx = 1\n"), {"": {"k": "a [s]", "x": "1"}})
        self.assertEqual(conf.parse("k = a \\\nb = c\n"), {"": {"k": "a b = c"}})
        self.assertEqual(conf.parse("k = a \\\n# b\nc = 1\n"), {"": {"k": "a", "c": "1"}})

    def test_5_comments_are_found_after_joining(self):
        self.assertEqual(self.value("k = a # b \\\nc"), "a")
        self.assertEqual(self.value("k = a \\\n#b"), "a")
        self.assertEqual(self.value("k = a#\\\nb"), "a# b")

    def test_5_empty_pieces(self):
        self.assertEqual(self.value("k = \\\n  b"), "b")
        self.assertEqual(conf.parse("k = a \\\n\nc = 1\n"), {"": {"k": "a", "c": "1"}})

    def test_5_backslash_on_the_last_line(self):
        self.fails("a = 1\nk = b \\", 2)
        self.fails("a = 1\nk = b \\\n", 2)
        self.fails("k = b \\\n c \\\n", 1)


class QuotedValues(Base):
    def test_6_kept_as_typed(self):
        self.assertEqual(self.value('k = "  a ; b # c  "'), "  a ; b # c  ")
        self.assertEqual(self.value('k = ""'), "")
        self.assertEqual(self.value('k = "a=b"   # c'), "a=b")
        self.assertEqual(self.value('k = "a" \t; c "d"'), "a")

    def test_6_escapes(self):
        self.assertEqual(self.value(r'k = "say \"hi\"\n\tC:\\dir"'), 'say "hi"\n\tC:\\dir')
        self.assertEqual(self.value(r'k = "caf\u00e9 \u4E2d"'), "caf\u00e9 \u4e2d")
        self.assertEqual(self.value(r'k = "\\"'), "\\")
        self.assertEqual(self.value(r'k = "\\n"'), "\\n")

    def test_6_errors(self):
        self.fails('a = 1\nk = "open\n', 2)
        self.fails('k = "a" b\n', 1)
        self.fails(r'k = "a\qb"' + "\n", 1)
        self.fails(r'k = "\u12"' + "\n", 1)
        self.fails(r'k = "\u12g4"' + "\n", 1)
        self.fails(r'k = "\x41"' + "\n", 1)
        self.fails('k = "a\\\nb"\n', 1)

    def test_6_plain_values_have_no_escapes(self):
        self.assertEqual(self.value(r"k = C:\new\table"), r"C:\new\table")
        self.assertEqual(self.value('k = say "hi"'), 'say "hi"')


class Lists(Base):
    def test_7_lists(self):
        got = conf.parse('[s]\nx[] = a # c\ny = 1\nx[] = " b "\n[t]\nx[] = c\n[s]\nx[] = d \\\n e\n')
        self.assertEqual(got, {"s": {"x": ["a", " b ", "d e"], "y": "1"}, "t": {"x": ["c"]}})
        self.assertEqual(list(got["s"]), ["x", "y"])

    def test_7_one_item(self):
        self.assertEqual(conf.parse("x[] = \n"), {"": {"x": [""]}})

    def test_7_mixing(self):
        self.fails("x = 1\ny = 2\nx[] = 3\n", 3)
        self.fails("x[] = 1\nx[] = 2\n\nx = 3\n", 4)
        self.assertEqual(conf.parse("x = 1\n[s]\nx[] = 2\n"), {"": {"x": "1"}, "s": {"x": ["2"]}})


class References(Base):
    def test_8_same_section_then_top(self):
        got = conf.parse("a = top\nb = B\n[s]\na = mine\nx = ${a}/${b}\n[t]\nx = ${a}\n")
        self.assertEqual(got["s"]["x"], "mine/B")
        self.assertEqual(got["t"]["x"], "top")

    def test_8_named_sections(self):
        got = conf.parse("a = top\n[s]\na = mine\nx = ${:a} ${s:a} ${t.u:a}\n[t.u]\na = far\n")
        self.assertEqual(got["s"]["x"], "top mine far")

    def test_8_forward_and_chained(self):
        got = conf.parse("[s]\nc = ${b}!\nb = ${a}${a}\na = x\n")
        self.assertEqual(got["s"], {"c": "xx!", "b": "xx", "a": "x"})

    def test_8_dollars(self):
        self.assertEqual(self.value("k = 5$$ or $5"), "5$ or $5")
        self.assertEqual(self.value("k = $$$$"), "$$")
        self.assertEqual(self.value("k = a $ b"), "a $ b")

    def test_8_replaced_text_is_not_scanned_again(self):
        got = conf.parse("a = $${b}\nb = no\nc = <${a}>\n")
        self.assertEqual(got[""], {"a": "${b}", "b": "no", "c": "<${b}>"})

    def test_8_quoted_and_lists(self):
        got = conf.parse('n = 4\nq = " ${n}\\t#${n} "\nx[] = ${n}a\nx[] = "${q}"\n')
        self.assertEqual(got[""], {"n": "4", "q": " 4\t#4 ", "x": ["4a", " 4\t#4 "]})

    def test_8_errors(self):
        self.fails("a = 1\nb = ${nope}\n", 2)
        self.fails("a = 1\n\nb = ${nope:a}\n", 3)
        self.fails("[s]\na = 1\n[t]\nb = ${a}\n", 4)
        self.fails("a = 1\nb = ${a\n", 2)
        self.fails("x[] = 1\ny = 2\nb = ${x}\n", 3)
        self.fails("x[] = 1\nx[] = ${nope}\n", 2)
        self.fails("a = 1\nb = x \\\n ${nope} \\\n y\n", 2)

    def test_8_cycles(self):
        self.fails("a = 1\nb = ${b}\n", 2)
        self.fails("a = ${b}\nb = ${c}\nc = ${a}\n", 1, 2, 3)
        self.fails("ok = 1\n[s]\na = ${t:b}\n[t]\nb = ${s:a}\n", 3, 5)

    def test_8_not_a_cycle(self):
        got = conf.parse("a = top\n[s]\na = ${:a}!\n")
        self.assertEqual(got["s"]["a"], "top!")


class Loading(Base):
    def test_example(self):
        settings = conf.load(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "app.conf"))
        self.assertEqual(settings[""], {"name": "reports", "root": "/srv/reports"})
        self.assertEqual(settings["server"]["banner"], "  reports ready\n")
        self.assertEqual(settings["paths"]["archive"], "/srv/reports/archive /srv/reports/cold")
        self.assertEqual(settings["mail"], {"subject": "Report #4: \u00fcbersicht", "footer": "costs 5$ per page"})


if __name__ == "__main__":
    unittest.main()
