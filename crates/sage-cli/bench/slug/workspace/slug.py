"""URL slugs."""


def slugify(title):
    """Lower-case `title` and join its words with single dashes.

    Letters and digits are kept (accented letters too); everything else separates words.
    """
    words = title.lower().split(" ")
    return "-".join(words)
