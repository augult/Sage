THOUGHT = "Use constrained permutations over five house positions, filtering categories in the order imposed by the clues."
CONTENT = ""
CALLS = [{"name": "edit", "arguments": {"path": "zebra_puzzle.py", "range": "1ti-6hc", "new": """from itertools import permutations

def _solve():
    positions = range(5)
    for colors in permutations(positions):
        red, green, ivory, yellow, blue = colors
        if green != ivory + 1:
            continue
        for nations in permutations(positions):
            english, spaniard, ukrainian, norwegian, japanese = nations
            if english != red or norwegian != 0 or abs(norwegian - blue) != 1:
                continue
            for drinks in permutations(positions):
                coffee, tea, milk, orange, water = drinks
                if coffee != green or tea != ukrainian or milk != 2:
                    continue
                for pets in permutations(positions):
                    dog, snail, fox, horse, zebra = pets
                    if dog != spaniard:
                        continue
                    for hobbies in permutations(positions):
                        dancing, painting, reading, football, chess = hobbies
                        if (snail != dancing or yellow != painting or
                            abs(reading - fox) != 1 or abs(painting - horse) != 1 or
                            football != orange or chess != japanese):
                            continue
                        residents = [None] * 5
                        for name, pos in zip(('Englishman', 'Spaniard', 'Ukrainian', 'Norwegian', 'Japanese'), nations):
                            residents[pos] = name
                        return residents[water], residents[zebra]

def drinks_water():
    return _solve()[0]

def owns_zebra():
    return _solve()[1]"""}}, {"name": "shell", "arguments": {"cmd": "python3 - <<'PY'\nfrom zebra_puzzle import drinks_water, owns_zebra\nassert drinks_water() == 'Norwegian'\nassert owns_zebra() == 'Japanese'\nprint('zebra puzzle checks passed')\nPY", "expect_exit": "0", "expect_has": "zebra puzzle checks passed"}}, {"name": "finish", "arguments": {"summary": "Implemented a constrained-permutation solver; the canonical water-drinker and zebra-owner checks pass.", "unverified": "The grader suite was not available."}}]
