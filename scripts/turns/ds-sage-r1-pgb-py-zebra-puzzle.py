THOUGHT = r'''A constraint-filtered permutation search is small enough here: fix the
Norwegian in house 1, require green immediately right of ivory, milk in the
middle house, and so on, pruning each category before moving to the next.
I'll cache the single solution and expose the two required answers from it
rather than hard-coding names.
'''

CONTENT = r'''Implementing a constrained search in `zebra_puzzle.py` that returns the water drinker and zebra owner.'''

CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "zebra_puzzle.py",
            "overwrite": True,
            "content": r"""from itertools import permutations

_NATIONS = ("Englishman", "Spaniard", "Ukrainian", "Norwegian", "Japanese")
_COLORS = ("red", "green", "ivory", "yellow", "blue")
_DRINKS = ("coffee", "tea", "milk", "orange juice", "water")
_HOBBIES = ("reading", "dancing", "painting", "football", "chess")
_PETS = ("dog", "snails", "fox", "horse", "zebra")

_solution = None


def drinks_water():
    return _solve()[0]


def owns_zebra():
    return _solve()[1]


def _solve():
    global _solution
    if _solution is None:
        _solution = _search()
    return _solution


def _search():
    for nations in permutations(_NATIONS):
        if nations[0] != "Norwegian":
            continue
        nation = {name: index for index, name in enumerate(nations)}
        for colors in permutations(_COLORS):
            color = {name: index for index, name in enumerate(colors)}
            if color["green"] != color["ivory"] + 1:
                continue
            if nation["Englishman"] != color["red"]:
                continue
            if abs(nation["Norwegian"] - color["blue"]) != 1:
                continue
            for drinks in permutations(_DRINKS):
                drink = {name: index for index, name in enumerate(drinks)}
                if drinks[2] != "milk":
                    continue
                if drink["coffee"] != color["green"]:
                    continue
                if drink["tea"] != nation["Ukrainian"]:
                    continue
                for hobbies in permutations(_HOBBIES):
                    hobby = {name: index for index, name in enumerate(hobbies)}
                    if hobby["painting"] != color["yellow"]:
                        continue
                    if hobby["football"] != drink["orange juice"]:
                        continue
                    if hobby["chess"] != nation["Japanese"]:
                        continue
                    for pets in permutations(_PETS):
                        pet = {name: index for index, name in enumerate(pets)}
                        if pet["dog"] != nation["Spaniard"]:
                            continue
                        if pet["snails"] != hobby["dancing"]:
                            continue
                        if abs(hobby["reading"] - pet["fox"]) != 1:
                            continue
                        if abs(hobby["painting"] - pet["horse"]) != 1:
                            continue
                        return nations[drink["water"]], nations[pet["zebra"]]
    raise ValueError("no solution")
""",
        },
    },
    {
        "name": "shell",
        "arguments": {
            "expect_exit": "0",
            "cmd": r"""python3 - <<'PY'
import time
import zebra_puzzle

start = time.time()
assert zebra_puzzle.drinks_water() == "Norwegian"
assert zebra_puzzle.owns_zebra() == "Japanese"
elapsed = time.time() - start
assert zebra_puzzle.drinks_water() == "Norwegian"
print("all checks passed in %.2fs" % elapsed)
PY""",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented `zebra_puzzle.py` with a cached permutation search over all 15 constraints; `drinks_water()` returns \"Norwegian\" and `owns_zebra()` returns \"Japanese\". Verified both answers and repeat calls in under a second.",
            "unverified": "",
        },
    },
]
