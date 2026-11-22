"""The parser for the .conf format (SPEC.md)."""

import re

from .errors import ConfError

_NAME = re.compile(r"[A-Za-z0-9_.-]+\Z")
_HEX = re.compile(r"[0-9A-Fa-f]{4}\Z")


def _valid_name(name):
    return bool(name) and _NAME.match(name) is not None


def _blank(text):
    return text.strip(" \t")


def _strip_comment(text):
    for index, char in enumerate(text):
        if char in "#;" and index > 0 and text[index - 1] in " \t":
            return text[:index].rstrip(" \t")
    return text


def _plain_value(first, lines, index):
    """Join a plain value's continuation lines; returns (value, next_line_index)."""
    start = index
    pieces = []
    piece_line = first
    while True:
        trimmed = piece_line.rstrip(" \t")
        if trimmed.endswith("\\"):
            piece = _blank(trimmed[:-1])
            if piece:
                pieces.append(piece)
            index += 1
            if index >= len(lines):
                raise ConfError("a continuation line is missing", start + 1)
            piece_line = lines[index]
            continue
        piece = _blank(piece_line)
        if piece:
            pieces.append(piece)
        break
    return _strip_comment(" ".join(pieces)), index + 1


def _quoted_value(text, index):
    """Unescape a quoted value; returns (value, next_line_index)."""
    number = index + 1
    chars = []
    position = 1
    closing = None
    while position < len(text):
        char = text[position]
        if char == '"':
            closing = position
            break
        if char != "\\":
            chars.append(char)
            position += 1
            continue
        if position + 1 >= len(text):
            raise ConfError("a backslash ends the line", number)
        escape = text[position + 1]
        simple = {"\\": "\\", '"': '"', "n": "\n", "t": "\t"}
        if escape in simple:
            chars.append(simple[escape])
            position += 2
            continue
        if escape == "u":
            digits = text[position + 2:position + 6]
            if _HEX.match(digits) is None:
                raise ConfError("a \\u escape needs four hexadecimal digits", number)
            chars.append(chr(int(digits, 16)))
            position += 6
            continue
        raise ConfError(f"unknown escape `\\{escape}`", number)
    if closing is None:
        raise ConfError("a quoted value is not closed", number)
    after = _blank(text[closing + 1:])
    if after and after[0] not in "#;":
        raise ConfError("text after a quoted value", number)
    return "".join(chars), index + 1


def _substitute(text, section, number, context):
    out = []
    position = 0
    while position < len(text):
        char = text[position]
        if char != "$":
            out.append(char)
            position += 1
            continue
        following = text[position + 1:position + 2]
        if following == "$":
            out.append("$")
            position += 2
            continue
        if following != "{":
            out.append("$")
            position += 1
            continue
        closing = text.find("}", position + 2)
        if closing == -1:
            raise ConfError("a reference is not closed", number)
        out.append(_lookup(text[position + 2:closing], section, number, context))
        position = closing + 1
    return "".join(out)


def _lookup(reference, section, number, context):
    sections = context["sections"]
    if ":" in reference:
        target, _, key = reference.partition(":")
    elif reference in sections.get(section, {}):
        target, key = section, reference
    else:
        target, key = "", reference
    if target not in sections:
        raise ConfError(f"no section `{target}`", number)
    if key not in sections[target]:
        raise ConfError(f"no key `{key}`", number)
    if context["lists"][(target, key)]:
        raise ConfError(f"`{key}` is a list", number)
    return _value(target, key, context)


def _value(section, key, context):
    identity = (section, key)
    if identity in context["done"]:
        return context["done"][identity]
    if identity in context["active"]:
        raise ConfError("a value refers to itself", context["lines"][identity])
    context["active"].add(identity)
    raw = context["sections"][section][key]
    number = context["lines"][identity]
    if context["lists"][identity]:
        result = [_substitute(item, section, number, context) for item in raw]
    else:
        result = _substitute(raw, section, number, context)
    context["active"].discard(identity)
    context["done"][identity] = result
    return result


def parse(text):
    """The sections of a .conf document: `{section: {key: value}}`."""
    if text.startswith("\ufeff"):
        text = text[1:]
    lines = text.replace("\r\n", "\n").split("\n")
    if lines and lines[-1] == "":
        lines.pop()

    sections = {}
    numbers = {}
    lists = {}
    current = None
    index = 0
    while index < len(lines):
        raw = lines[index]
        stripped = _blank(raw)
        number = index + 1
        if not stripped or stripped[0] in "#;":
            index += 1
            continue
        if stripped[0] == "[":
            if not stripped.endswith("]"):
                raise ConfError("a section header is not closed", number)
            name = _blank(stripped[1:-1])
            if not _valid_name(name):
                raise ConfError("a bad section name", number)
            sections.setdefault(name, {})
            current = name
            index += 1
            continue
        if "=" not in raw:
            raise ConfError("expected `key = value`", number)
        key_part, _, rest = raw.partition("=")
        key_part = _blank(key_part)
        is_list = key_part.endswith("[]")
        key = key_part[:-2] if is_list else key_part
        key = _blank(key)
        if not _valid_name(key):
            raise ConfError("a bad key", number)
        section = "" if current is None else current
        value_text = rest.strip(" \t")
        if value_text.startswith('"'):
            value, index = _quoted_value(value_text, index)
        else:
            value, index = _plain_value(value_text, lines, index)
        identity = (section, key)
        if identity in numbers and not (is_list and lists[identity]):
            raise ConfError("a duplicate key", number)
        numbers[identity] = number
        lists[identity] = is_list
        target = sections.setdefault(section, {})
        if is_list:
            target.setdefault(key, []).append(value)
        else:
            target[key] = value

    if not sections.get(""):
        sections.pop("", None)

    context = {
        "sections": sections,
        "lines": numbers,
        "lists": lists,
        "done": {},
        "active": set(),
    }
    result = {}
    for name, entries in sections.items():
        resolved = {}
        for key in entries:
            resolved[key] = _value(name, key, context)
        result[name] = resolved
    return result


def load(path):
    with open(path, encoding="utf-8") as source:
        return parse(source.read())
