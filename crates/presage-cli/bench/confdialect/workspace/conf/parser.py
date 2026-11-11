"""The parser for the .conf format (SPEC.md).

First cut: sections, entries and whole-line comments. Inline comments, continuation lines,
quoting, lists and references are still missing, and nothing is validated yet.
"""

from .errors import ConfError


def parse(text):
    """The sections of a .conf document: `{section: {key: value}}`."""
    sections = {}
    current = sections.setdefault("", {})
    for number, line in enumerate(text.split("\n"), 1):
        line = line.strip()
        if not line or line[0] in "#;":
            continue
        if line.startswith("[") and line.endswith("]"):
            current = sections.setdefault(line[1:-1].strip(), {})
            continue
        if "=" not in line:
            raise ConfError("expected `key = value`", number)
        key, _, value = line.partition("=")
        current[key.strip()] = value.strip()
    if not sections[""]:
        del sections[""]
    return sections


def load(path):
    with open(path, encoding="utf-8") as source:
        return parse(source.read())
