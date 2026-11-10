"""Small text statistics helpers."""
import re

_WORD = re.compile(r"[a-z']+")


def words(text):
    """Lower-cased words of `text`, in order."""
    return _WORD.findall(text.lower())


def word_count(text):
    """How many words `text` has."""
    return len(words(text))
