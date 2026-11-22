THOUGHT = r'''`drinks_water` and `owns_zebra` are stubs. I search the 5x5 grid by
permuting each category and rejecting assignments that break a clue, applying
the strongest prunes early (Norwegian in house 1 constrains blue to house 2,
green right of ivory, milk in the middle, etc.). The single consistent
assignment answers both questions; caching keeps the second call free.'''

CONTENT = r'''Adding a permutation-based solver with the fifteen clues as constraints, then
checking the water drinker and zebra owner against the known answers.'''

CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "zebra_puzzle.py",
            "overwrite": True,
            "content": r'''from functools import lru_cache
from itertools import permutations

NATIONALS = ["Englishman", "Spaniard", "Ukrainian", "Norwegian", "Japanese"]
COLORS = ["red", "green", "ivory", "yellow", "blue"]
DRINKS = ["coffee", "tea", "milk", "orange juice", "water"]
HOBBIES = ["dancing", "painter", "reading", "football", "chess"]
PETS = ["dog", "snail", "fox", "horse", "zebra"]


@lru_cache(maxsize=1)
def _solution():
    for national in permutations(NATIONALS):
        if national[0] != "Norwegian":
            continue
        norwegian = national.index("Norwegian")
        englishman = national.index("Englishman")
        spaniard = national.index("Spaniard")
        ukrainian = national.index("Ukrainian")
        japanese = national.index("Japanese")
        for color in permutations(COLORS):
            if color[englishman] != "red":
                continue
            if color.index("green") != color.index("ivory") + 1:
                continue
            if abs(color.index("blue") - norwegian) != 1:
                continue
            green = color.index("green")
            yellow = color.index("yellow")
            for drink in permutations(DRINKS):
                if drink[2] != "milk" or drink[green] != "coffee":
                    continue
                if drink[ukrainian] != "tea":
                    continue
                orange = drink.index("orange juice")
                for hobby in permutations(HOBBIES):
                    if hobby[yellow] != "painter":
                        continue
                    if hobby[orange] != "football":
                        continue
                    if hobby[japanese] != "chess":
                        continue
                    dancing = hobby.index("dancing")
                    reading = hobby.index("reading")
                    painter = hobby.index("painter")
                    for pet in permutations(PETS):
                        if pet[spaniard] != "dog":
                            continue
                        if pet[dancing] != "snail":
                            continue
                        if abs(pet.index("fox") - reading) != 1:
                            continue
                        if abs(pet.index("horse") - painter) != 1:
                            continue
                        return {
                            "national": national,
                            "color": color,
                            "drink": drink,
                            "hobby": hobby,
                            "pet": pet,
                        }
    raise ValueError("no solution")


def drinks_water():
    solution = _solution()
    return solution["national"][solution["drink"].index("water")]


def owns_zebra():
    solution = _solution()
    return solution["national"][solution["pet"].index("zebra")]
''',
        },
    },
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''cat > /tmp/check_zebra.py <<'EOF'
import os
import sys
sys.path.insert(0, os.getcwd())
from zebra_puzzle import drinks_water, owns_zebra

assert drinks_water() == "Norwegian", drinks_water()
assert owns_zebra() == "Japanese", owns_zebra()
print("all checks passed")
EOF
python3 /tmp/check_zebra.py; rc=$?; rm -f /tmp/check_zebra.py; exit $rc''',
            "expect_has": "all checks passed",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented `zebra_puzzle.py` with a cached permutation search over the five categories, encoding all fifteen clues as constraints (Norwegian in house 1 next to blue, green immediately right of ivory, milk in the middle, colour/nationality/drink/hobby/pet pairings and the two neighbour relations). `drinks_water()` returns the water drinker and `owns_zebra()` the zebra owner. Verified with a Python check asserting `drinks_water() == 'Norwegian'` and `owns_zebra() == 'Japanese'` (all checks passed).",
            "unverified": "",
        },
    },
]
