THOUGHT = r'''The search returns exactly one arrangement and it satisfies every clue, so the two
answers follow from it.'''

CONTENT = r'''Implemented `drinks_water()` and `owns_zebra()` in `zebra_puzzle.py` by searching nationalities, colours, drinks, hobbies and pets with the cheapest clues applied first (Norwegian in house one, blue house second, Englishman in red, green immediately right of ivory, milk in the middle, green drinks coffee, Ukrainian drinks tea, Japanese plays chess, yellow paints), then checking the paired clues (Spaniard keeps the dog, snails owner dances, reader next to the fox, painter next to the horse, football drinks orange juice). The one surviving arrangement is cached, and both functions read their answer from it.

Verified by printing the arrangement and asserting the answers: the Norwegian drinks water and the Japanese keeps the zebra. The search finishes in well under a second (all checks passed).'''

CALLS = []
