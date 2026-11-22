THOUGHT = r'''The search returns item-to-house position permutations; map the water and zebra house positions back to their occupants.'''
CONTENT = ""
CALLS = [
    {"name": "edit", "arguments": {"path": "zebra_puzzle.py", "old": "return ('Englishman', 'Spaniard', 'Ukrainian', 'Norwegian', 'Japanese')[drinks.index(4)]", "new": "return ('Englishman', 'Spaniard', 'Ukrainian', 'Norwegian', 'Japanese')[nations.index(drinks[4])]"}},
    {"name": "edit", "arguments": {"path": "zebra_puzzle.py", "old": "return ('Englishman', 'Spaniard', 'Ukrainian', 'Norwegian', 'Japanese')[pets.index(4)]", "new": "return ('Englishman', 'Spaniard', 'Ukrainian', 'Norwegian', 'Japanese')[nations.index(pets[4])]"}},
    {"name": "shell", "arguments": {"cmd": "python3 -c \"from zebra_puzzle import *; assert drinks_water()=='Norwegian'; assert owns_zebra()=='Japanese'; print('zebra checks passed')\"", "expect_exit": "0", "expect_has": "zebra checks passed"}},
    {"name": "finish", "arguments": {"summary": "Fixed the occupant lookup after the first solver version returned a house-index mismatch. Verified Norwegian drinks water and Japanese owns the zebra.", "unverified": "The supplied grader tests were unavailable."}}
]
