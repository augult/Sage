"""Small text statistics helpers."""
import re
from collections import Counter

_WORD = re.compile(r"[a-z']+")


def words(text):
    """Lower-cased words of `text`, in order."""
    return _WORD.findall(text.lower())


def word_count(text):
    """How many words `text` has."""
    return len(words(text))


def top_words(text, n=3):
    """The `n` most frequent words as (word, count) pairs; ties are ordered alphabetically."""
    if n < 1:
        raise ValueError("n must be at least 1")
    counts = Counter(words(text))
    return sorted(counts.items(), key=lambda pair: (-pair[1], pair[0]))[:n]
