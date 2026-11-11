"""Helpers from when stockr still ran on Python 2."""

import sys

PY2 = sys.version_info[0] == 2

text_type = str
string_types = (str,)


def iteritems(mapping):
    return iter(mapping.items())


def to_text(value, encoding="utf-8"):
    if isinstance(value, bytes):
        return value.decode(encoding)
    return str(value)
