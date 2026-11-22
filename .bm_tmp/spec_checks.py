import sys

sys.path.insert(0, "/tmp/confwork")

import conf
from conf import ConfError

FAILS = []


def check(name, got, want):
    if got != want:
        FAILS.append(f"{name}: got {got!r} want {want!r}")


def bad(name, text, line):
    try:
        conf.parse(text)
    except ConfError as error:
        if error.line != line:
            FAILS.append(f"{name}: error line {error.line} want {line}")
        return
    FAILS.append(f"{name}: no error")


check("top only", conf.parse("a = 1\n"), {"": {"a": "1"}})
check("empty", conf.parse(""), {})
check("empty section kept", conf.parse("[a]\n[b]\nk=1\n"), {"a": {}, "b": {"k": "1"}})
check("merge sections", conf.parse("[a]\nx=1\n[b]\ny=2\n[a]\nz=3\n"), {"a": {"x": "1", "z": "3"}, "b": {"y": "2"}})
check("bom", conf.parse("\ufeffa = 1\n"), {"": {"a": "1"}})
check("crlf", conf.parse("[s]\r\nk = v\r\n"), {"s": {"k": "v"}})
check("spaces header", conf.parse("  [ db.primary ]  \nk=1\n"), {"db.primary": {"k": "1"}})
check("empty value", conf.parse("k =\n"), {"": {"k": ""}})
check("equals in value", conf.parse("k = a=b=c\n"), {"": {"k": "a=b=c"}})
check("plain comment", conf.parse("path = /srv/app   # where it lives\n"), {"": {"path": "/srv/app"}})
check("value starts hash", conf.parse("colour = #ff8800\n"), {"": {"colour": "#ff8800"}})
check("hash inside", conf.parse("tag = a#b ;c\n"), {"": {"tag": "a#b"}})
check("hash no space", conf.parse("note = # not a comment\n"), {"": {"note": "# not a comment"}})
check("cont", conf.parse("hosts = alpha \\\n        beta \\\n        gamma   # three of them\n"), {"": {"hosts": "alpha beta gamma"}})
check("cont blank piece", conf.parse("h = a \\\n \\\n b\n"), {"": {"h": "a b"}})
check("cont looks comment", conf.parse("h = a \\\n#b\n"), {"": {"h": "a"}})
check("cont looks header", conf.parse("h = a \\\n[x]\n"), {"": {"h": "a [x]"}})
check("cont equals", conf.parse("h = a \\\nb=c\n"), {"": {"h": "a b=c"}})
check("quoted", conf.parse('g = "  hello # world  "   ; kept\n'), {"": {"g": "  hello # world  "}})
check("quoted escapes", conf.parse('k = "a\\\\b\\"c\\nd\\te"\n'), {"": {"k": 'a\\b"c\nd\te'}})
check("quoted unicode", conf.parse('k = "\\u00fc"\n'), {"": {"k": "ü"}})
check("quoted comment", conf.parse('k = "v" # tail\n'), {"": {"k": "v"}})
check("quoted semicolon comment", conf.parse('k = "v" ; tail\n'), {"": {"k": "v"}})
check("lists", conf.parse("[s]\nx[] = 1\nx[] = 2\n"), {"s": {"x": ["1", "2"]}})
check("list refs items", conf.parse("b = /s\n[s]\nx[] = ${b}/1\nx[] = \"${b}/2\"\n"), {"": {"b": "/s"}, "s": {"x": ["/s/1", "/s/2"]}})
check("ref same sec", conf.parse("b=1\n[s]\na = ${b}\n"), {"": {"b": "1"}, "s": {"a": "1"}})
check("ref own sec", conf.parse("[s]\na=1\nb=${a}x\n"), {"s": {"a": "1", "b": "1x"}})
check("ref sec", conf.parse("[s]\na=1\n[t]\nb=${s:a}\n"), {"s": {"a": "1"}, "t": {"b": "1"}})
check("ref top explicit", conf.parse("a=1\n[t]\nb=${:a}\n"), {"": {"a": "1"}, "t": {"b": "1"}})
check("forward ref", conf.parse("a=${b}\nb=2\n"), {"": {"a": "2", "b": "2"}})
check("chain", conf.parse("a=${b}\nb=${c}\nc=3\n"), {"": {"a": "3", "b": "3", "c": "3"}})
check("no rescan", conf.parse("a=$${b}\nb=1\n"), {"": {"a": "${b}", "b": "1"}})
check("dollar", conf.parse("a = 5$$ or $5\n"), {"": {"a": "5$ or $5"}})
check("dollar end", conf.parse("a = x$\n"), {"": {"a": "x$"}})
check("dollar other", conf.parse("a = $x\n"), {"": {"a": "$x"}})

bad("header comment", "[a] # no\n", 1)
bad("header junk", "[a] x\n", 1)
bad("header unclosed", "[a\n", 1)
bad("header empty", "[]\n", 1)
bad("header bad char", "[a/b]\nk=1\n", 1)
bad("no equals", "a=1\nnonsense\n", 2)
bad("bad key", "a b = 1\n", 1)
bad("empty key", "= 1\n", 1)
bad("bad key char", "a[b] = 1\n", 1)
bad("dup key", "[s]\na=1\na=2\n", 3)
bad("dup scalar then list", "[s]\na=1\na[]=2\n", 3)
bad("dup list then scalar", "[s]\na[]=2\na=1\n", 3)
bad("bad escape", 'k = "a\\q"\n', 1)
bad("short unicode", 'k = "\\u12"\n', 1)
bad("unclosed quote", 'k = "abc\n', 1)
bad("text after quote", 'k = "abc" x\n', 1)
bad("backslash eof", "k = a \\\n", 1)
check("backslash then blank", conf.parse("k = a \\\n\n"), {"": {"k": "a"}})
bad("missing key", "a=${nope}\n", 1)
bad("missing section", "a=${nope:k}\n", 1)
bad("missing top", "[s]\na=${:nope}\n", 2)
bad("ref list", "[s]\nx[]=1\ny=${x}\n", 3)
bad("ref list qualified", "[s]\nx[]=1\n[t]\ny=${s:x}\n", 4)
bad("unclosed ref", "a=${b\n", 1)
bad("self ref", "a=${a}\n", 1)
bad("cycle", "a=${b}\nb=${a}\n", 1)
bad("cycle 3", "a=${b}\nb=${c}\nc=${a}\n", 1)
bad("ref line continued", "a = x \\\n ${nope}\n", 1)

if FAILS:
    print("\n".join(FAILS))
    sys.exit(1)
print("all spec checks passed")
