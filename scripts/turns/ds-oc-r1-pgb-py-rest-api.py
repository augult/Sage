THOUGHT = r'''The suite passes across empty and pre-seeded databases, creation, netting,
overpayment, exact payoff, filtering and multi-party balances, so the exercise is
implemented as specified.'''

CONTENT = r'''Implemented `rest_api.py`: `get('/users')` returns a JSON string of every user, or the requested subset, sorted by name with `owes`, `owed_by` and a computed `balance`; `post('/add')` creates a user; `post('/iou')` records a loan and nets it against debt in the opposite direction, so a pair never owes each other, a repayment clears both sides, and an overpayment flips who owes whom.

Debt is stored as a pairwise ledger (`net[a][b]` is what `a` is owed by `b`), which keeps every case an addition. Verified with a scenario script covering the empty database, user creation, both netting directions, overpayment, exact payoff, filtered listing sorted by name, a pre-seeded database and a four-person balance (all checks passed).'''

CALLS = []
