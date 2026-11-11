"""The parser for the .conf format (SPEC.md)."""

import re

from .errors import ConfError

BLANKS = " \t"
NAME = re.compile(r"[A-Za-z0-9_.-]+\Z")
ESCAPES = {"\\": "\\", '"': '"', "n": "\n", "t": "\t"}
HEX = re.compile(r"[0-9A-Fa-f]{4}\Z")


def _strip_comment(value):
    """A plain value without its inline comment (§4)."""
    for i in range(1, len(value)):
        if value[i] in "#;" and value[i - 1] in BLANKS:
            return value[:i].rstrip(BLANKS)
    return value


def _unquote(value, line):
    """The text of a quoted value (§6)."""
    out = []
    i = 1
    while i < len(value):
        ch = value[i]
        if ch == '"':
            rest = value[i + 1 :].lstrip(BLANKS)
            if rest and rest[0] not in "#;":
                raise ConfError("unexpected text after the closing quote", line)
            return "".join(out)
        if ch == "\\":
            code = value[i + 1 : i + 2]
            if code in ESCAPES:
                out.append(ESCAPES[code])
                i += 2
            elif code == "u" and HEX.match(value[i + 2 : i + 6]):
                out.append(chr(int(value[i + 2 : i + 6], 16)))
                i += 6
            else:
                raise ConfError(f"unknown escape \\{code}", line)
            continue
        out.append(ch)
        i += 1
    raise ConfError("missing closing quote", line)


def _read(text):
    """The entries of a document: `(section, key, is_list, raw value, line)`, and the sections."""
    if text.startswith("\ufeff"):
        text = text[1:]
    lines = [line[:-1] if line.endswith("\r") else line for line in text.split("\n")]
    if lines[-1] == "":
        lines.pop()
    sections = {}
    entries = []
    kinds = {}
    section = ""
    i = 0
    while i < len(lines):
        number = i + 1
        line = lines[i].strip(BLANKS)
        i += 1
        if not line or line[0] in "#;":
            continue
        if line[0] == "[":
            name = line[1:-1].strip(BLANKS)
            if not line.endswith("]") or not NAME.match(name):
                raise ConfError("malformed section header", number)
            section = name
            sections.setdefault(section, {})
            continue
        if "=" not in line:
            raise ConfError("expected `key = value`", number)
        key, _, value = line.partition("=")
        key = key.strip(BLANKS)
        value = value.strip(BLANKS)
        is_list = key.endswith("[]")
        if is_list:
            key = key[:-2]
        if not NAME.match(key):
            raise ConfError(f"malformed key {key!r}", number)
        if value.startswith('"'):
            value = _unquote(value, number)
        else:
            pieces = []
            while value.endswith("\\"):
                pieces.append(value[:-1].strip(BLANKS))
                if i >= len(lines):
                    raise ConfError("continuation at the end of the file", number)
                value = lines[i].strip(BLANKS)
                i += 1
            pieces.append(value)
            value = _strip_comment(" ".join(piece for piece in pieces if piece))
        kind = kinds.setdefault((section, key), is_list)
        if kind != is_list:
            raise ConfError(f"{key} is both a value and a list", number)
        if not is_list and key in sections.setdefault(section, {}):
            raise ConfError(f"duplicate key {key}", number)
        if is_list:
            sections.setdefault(section, {}).setdefault(key, [])
        else:
            sections.setdefault(section, {})[key] = None
        entries.append((section, key, is_list, value, number))
    return sections, entries


class _Resolver:
    def __init__(self, entries):
        self.entries = {}
        self.lists = set()
        for section, key, is_list, value, line in entries:
            if is_list:
                self.lists.add((section, key))
            else:
                self.entries[(section, key)] = (value, line)
        self.done = {}
        self.active = set()

    def value_of(self, section, key, line):
        """The resolved value of a plain entry, for a reference on `line`."""
        where = (section, key)
        if where in self.lists:
            raise ConfError(f"reference to the list {key}", line)
        if where not in self.entries:
            raise ConfError(f"reference to unknown key {key}", line)
        if where in self.done:
            return self.done[where]
        if where in self.active:
            raise ConfError(f"{key} refers to itself", line)
        self.active.add(where)
        raw, own_line = self.entries[where]
        self.done[where] = self.resolve(raw, section, own_line)
        self.active.discard(where)
        return self.done[where]

    def resolve(self, raw, section, line):
        """`raw` with its references replaced (§8)."""
        out = []
        i = 0
        while i < len(raw):
            if raw[i] != "$":
                out.append(raw[i])
                i += 1
            elif raw.startswith("$$", i):
                out.append("$")
                i += 2
            elif raw.startswith("${", i):
                end = raw.find("}", i)
                if end < 0:
                    raise ConfError("`${` without `}`", line)
                name = raw[i + 2 : end]
                if ":" in name:
                    target, _, key = name.partition(":")
                else:
                    key = name
                    target = section if (section, key) in self.entries or (section, key) in self.lists else ""
                out.append(self.value_of(target, key, line))
                i = end + 1
            else:
                out.append("$")
                i += 1
        return "".join(out)


def parse(text):
    """The sections of a .conf document: `{section: {key: value}}`."""
    sections, entries = _read(text)
    resolver = _Resolver(entries)
    for section, key, is_list, value, line in entries:
        if is_list:
            sections[section][key].append(resolver.resolve(value, section, line))
        else:
            sections[section][key] = resolver.value_of(section, key, line)
    if "" in sections and not sections[""]:
        del sections[""]
    return sections


def load(path):
    with open(path, encoding="utf-8") as source:
        return parse(source.read())
