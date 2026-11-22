THOUGHT = r'''`recite(start, end)` returns the lyric lines from verse `start` through `end`, inclusive, with a blank line between verses. The repetition is regular: each verse has its own second line, then the chain of "She swallowed the X to catch the Y." lines walking back down the animals (the spider line carries its description), ending in the fly refrain; the horse verse ends early. I'll generate it from two tables and check the exact verses against the README.'''
CONTENT = ""
CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "food_chain.py",
            "overwrite": True,
            "content": r'''ANIMALS = ("fly", "spider", "bird", "cat", "dog", "goat", "cow", "horse")

SECOND_LINES = {
    1: "I don't know why she swallowed the fly. Perhaps she'll die.",
    2: "It wriggled and jiggled and tickled inside her.",
    3: "How absurd to swallow a bird!",
    4: "Imagine that, to swallow a cat!",
    5: "What a hog, to swallow a dog!",
    6: "Just opened her throat and swallowed a goat!",
    7: "I don't know how she swallowed a cow!",
    8: "She's dead, of course!",
}

SPIDER_LINE = (
    "She swallowed the bird to catch the spider "
    "that wriggled and jiggled and tickled inside her."
)


def verse(number):
    animal = ANIMALS[number - 1]
    lines = [
        f"I know an old lady who swallowed a {animal}.",
        SECOND_LINES[number],
    ]
    if number == len(ANIMALS):
        return lines
    for index in range(number - 1, 1, -1):
        if ANIMALS[index] == "bird":
            lines.append(SPIDER_LINE)
        else:
            lines.append(f"She swallowed the {ANIMALS[index]} to catch the {ANIMALS[index - 1]}.")
    if number > 1:
        lines.append("She swallowed the spider to catch the fly.")
    if number != 1:
        lines.append(SECOND_LINES[1])
    return lines


def recite(start_verse, end_verse):
    lines = []
    for number in range(start_verse, end_verse + 1):
        if lines:
            lines.append("")
        lines.extend(verse(number))
    return lines
''',
        },
    },
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''python3 - <<'PY'
from food_chain import recite

assert recite(1, 1) == [
    "I know an old lady who swallowed a fly.",
    "I don't know why she swallowed the fly. Perhaps she'll die.",
]
assert recite(3, 3) == [
    "I know an old lady who swallowed a bird.",
    "How absurd to swallow a bird!",
    "She swallowed the bird to catch the spider that wriggled and jiggled and tickled inside her.",
    "She swallowed the spider to catch the fly.",
    "I don't know why she swallowed the fly. Perhaps she'll die.",
]
assert recite(8, 8) == [
    "I know an old lady who swallowed a horse.",
    "She's dead, of course!",
]
assert recite(7, 7) == [
    "I know an old lady who swallowed a cow.",
    "I don't know how she swallowed a cow!",
    "She swallowed the cow to catch the goat.",
    "She swallowed the goat to catch the dog.",
    "She swallowed the dog to catch the cat.",
    "She swallowed the cat to catch the bird.",
    "She swallowed the bird to catch the spider that wriggled and jiggled and tickled inside her.",
    "She swallowed the spider to catch the fly.",
    "I don't know why she swallowed the fly. Perhaps she'll die.",
]
assert recite(2, 2)[:1] == ["I know an old lady who swallowed a spider."]
assert recite(1, 2)[2] == ""
assert recite(1, 8)[-2:] == ["I know an old lady who swallowed a horse.", "She's dead, of course!"]
assert recite(1, 8).count("I don't know why she swallowed the fly. Perhaps she'll die.") == 7
assert recite(5, 6)[-1] == "I don't know why she swallowed the fly. Perhaps she'll die."
print("ALL PASS")
PY''',
            "expect_exit": "0",
            "expect_has": "ALL PASS",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": r'''Implemented `recite(start_verse, end_verse)` in food_chain.py. Verses are generated from an animal order plus per-verse second lines: each verse gets its intro and second line, then the chain of "She swallowed the X to catch the Y." lines walking back to the spider (whose catch line keeps its description) and the fly, closing with the fly refrain. The horse verse stops after "She's dead, of course!", and verses in the returned list are separated by a blank line.

Verified with an inline check of the exact verse 1, 3, 7 and 8 text from the README, blank-line separation, the ending of a 1-8 recital, and the refrain appearing seven times. All passed.'''
        },
    },
]
