THOUGHT = r'''The scenario confirms empty database, add, netting in both directions, full payoff,
overpayment flip, filtered sorted listing and a pre-seeded database. Done.'''

CONTENT = r'''Implemented `rest_api.py`: `get('/users')` returns a JSON string with every user, or the requested subset, sorted by name; `post('/add')` creates a user; `post('/iou')` records a loan against a pairwise ledger, so opposite debts net out, a full repayment clears both sides, and an overpayment flips who owes whom.

Verified with a scenario script covering the empty database, user creation, partial and full repayment, overpayment, filtered listing and a pre-seeded database (all checks passed).'''

CALLS = []
