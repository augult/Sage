THOUGHT = "Represent each verse's animal and catch lines as data, and compose the requested verse range."
CONTENT = ""
CALLS = [
    {"name": "edit", "arguments": {"path": "food_chain.py", "range": "1mi-2hc", "new": r'''_ANIMALS = ["fly", "spider", "bird", "cat", "dog", "goat", "cow", "horse"]
_OPENERS = [
    "I know an old lady who swallowed a fly.",
    "I know an old lady who swallowed a spider.",
    "I know an old lady who swallowed a bird.",
    "I know an old lady who swallowed a cat.",
    "I know an old lady who swallowed a dog.",
    "I know an old lady who swallowed a goat.",
    "I know an old lady who swallowed a cow.",
    "I know an old lady who swallowed a horse.",
]
_COMMENTS = [
    "", "It wriggled and jiggled and tickled inside her.", "How absurd to swallow a bird!",
    "Imagine that, to swallow a cat!", "What a hog, to swallow a dog!",
    "Just opened her throat and swallowed a goat!", "I don't know how she swallowed a cow!",
    "She's dead, of course!",
]
_CATCH = [
    "", "the spider that wriggled and jiggled and tickled inside her", "the bird",
    "the cat", "the dog", "the goat", "the cow",
]

def recite(start_verse, end_verse):
    verses = []
    for n in range(start_verse, end_verse + 1):
        i = n - 1
        lines = [_OPENERS[i]]
        if _COMMENTS[i]:
            lines.append(_COMMENTS[i])
        if i == 7:
            verses.append("\n".join(lines))
            continue
        for animal in range(i, 0, -1):
            lines.append(f"She swallowed the {_ANIMALS[animal]} to catch {_CATCH[animal]}.")
        lines.append("She swallowed the spider to catch the fly.")
        lines.append("I don't know why she swallowed the fly. Perhaps she'll die.")
        verses.append("\n".join(lines))
    return "\n\n".join(verses)'''}},
    {"name": "shell", "arguments": {"cmd": "python3 -m py_compile food_chain.py && python3 - <<'PY'\nfrom food_chain import recite\ns=recite(1,2)\nassert s.startswith('I know an old lady who swallowed a fly.')\nassert 'She swallowed the spider to catch the fly.' in s\nassert recite(8,8) == \"I know an old lady who swallowed a horse.\\nShe's dead, of course!\"\nassert len(recite(1,8).split('\\n\\n')) == 8\nPY", "expect_exit": "0", "timeout": 30}},
    {"name": "finish", "arguments": {"summary": "Implemented cumulative song recitation for requested verse ranges, including the horse finale. Compilation and focused range checks pass.", "unverified": "Hidden grader tests are unavailable."}}
]
