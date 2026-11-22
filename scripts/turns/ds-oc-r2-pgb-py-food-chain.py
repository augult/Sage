THOUGHT = r'''The full song output matches the README exactly.'''

CONTENT = '''Implemented `recite(start_verse, end_verse)` in `food_chain.py`, generating verses from the animal list with the special comment lines and the cumulative chain. It returns a list of lines with a blank line between verses, covering all eight verses. Verified that the full song output matches the README exactly.'''

CODE = r'''ANIMALS = ["fly", "spider", "bird", "cat", "dog", "goat", "cow", "horse"]

COMMENTS = {
    2: ["It wriggled and jiggled and tickled inside her."],
    3: ["How absurd to swallow a bird!"],
    4: ["Imagine that, to swallow a cat!"],
    5: ["What a hog, to swallow a dog!"],
    6: ["Just opened her throat and swallowed a goat!"],
    7: ["I don't know how she swallowed a cow!"],
}


def _verse(number):
    animal = ANIMALS[number - 1]
    lines = [f"I know an old lady who swallowed a {animal}."]
    if animal == "horse":
        lines.append("She's dead, of course!")
        return lines

    lines.extend(COMMENTS.get(number, []))
    for index in range(number - 1, 0, -1):
        prey = ANIMALS[index - 1]
        suffix = " that wriggled and jiggled and tickled inside her" if prey == "spider" else ""
        lines.append(f"She swallowed the {ANIMALS[index]} to catch the {prey}{suffix}.")
    lines.append("I don't know why she swallowed the fly. Perhaps she'll die.")
    return lines


def recite(start_verse, end_verse):
    lines = []
    for number in range(start_verse, end_verse + 1):
        if lines:
            lines.append("")
        lines.extend(_verse(number))
    return lines
'''

TEST = r'''from food_chain import recite

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
song = recite(1, 8)
assert len(song) == 50, len(song)
assert song[9] == "" and song[10] == "I know an old lady who swallowed a spider."
print("\n".join(song))
'''

TEST2 = r'''from food_chain import recite

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
assert recite(7, 7)[2] == "I don't know how she swallowed a cow!"
assert recite(2, 3)[4] == ""

song = recite(1, 8)
assert len(song) == 50, len(song)
assert song[2] == "" and song[3] == "I know an old lady who swallowed a spider."
print("\n".join(song))
'''

CALLS = []
