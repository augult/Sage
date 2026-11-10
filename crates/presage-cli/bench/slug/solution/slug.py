"""URL slugs."""
import re

_WORD = re.compile(r"[^\W_]+")


def slugify(title):
    """Lower-case `title` and join its words with single dashes.

    Letters and digits are kept (accented letters too); everything else separates words.
    """
    return "-".join(_WORD.findall(title.lower()))
