THOUGHT = r'''The implementation is fine; the failing assertion was a bogus length formula I wrote (5 * 0). Replacing it with an exact comparison of the whole song against the README lyrics block, which is the real specification.'''
CONTENT = ""
CALLS = [
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''python3 - <<'PY'
from food_chain import recite

fly = [
    "I know an old lady who swallowed a fly.",
    "I don't know why she swallowed the fly. Perhaps she'll die.",
]
assert recite(1, 1) == fly, recite(1, 1)

spider = [
    "I know an old lady who swallowed a spider.",
    "It wriggled and jiggled and tickled inside her.",
    "She swallowed the spider to catch the fly.",
    "I don't know why she swallowed the fly. Perhaps she'll die.",
]
assert recite(2, 2) == spider, recite(2, 2)

bird = [
    "I know an old lady who swallowed a bird.",
    "How absurd to swallow a bird!",
    "She swallowed the bird to catch the spider that wriggled and jiggled and tickled inside her.",
    "She swallowed the spider to catch the fly.",
    "I don't know why she swallowed the fly. Perhaps she'll die.",
]
assert recite(3, 3) == bird, recite(3, 3)

assert recite(8, 8) == [
    "I know an old lady who swallowed a horse.",
    "She's dead, of course!",
]

cow = recite(7, 7)
assert cow[1] == "I don't know how she swallowed a cow!"
assert cow[2] == "She swallowed the cow to catch the goat."
assert len(cow) == 9, cow

assert recite(1, 2) == fly + [""] + spider

fence = chr(96) * 3 + "text"
readme = open("README.md").read()
start = readme.index(fence) + len(fence)
start = readme.index("\n", start) + 1
end = readme.index(chr(96) * 3, start)
expected = readme[start:end].rstrip("\n")
assert "\n".join(recite(1, 8)) == expected
assert recite(1, 8).count("") == 7
print("ALL PASS")
PY''',
            "expect_exit": "0",
            "expect_has": "ALL PASS",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented food_chain.py: verse(n) builds each stanza from an animal table, a per-animal remark, and a swallow chain walking back to the fly (with the spider's long description), and the horse verse ends immediately; recite(start, end) returns the flat list of lyric lines with one blank line between verses. Verified by comparing the whole song text from recite(1, 8) against the README's lyrics block exactly, plus individual fly/spider/bird/cow/horse verses and the two-verse join.",
            "unverified": "",
        },
    },
]
