THOUGHT = r'''The implementation passed the scripted checks on the first run: creation,
filtered and unfiltered listings, netting in both directions, balance signs,
and pre-seeded databases. Nothing else is outstanding.'''

CONTENT = r'''`rest_api.py` now implements the IOU API. `GET /users` lists every user sorted by name, or only the names in the payload, each as `{name, owes, owed_by, balance}` with `balance` = total owed to them minus what they owe. `POST /add` creates a unique user, and `POST /iou` records a loan between lender and borrower, netting it against any debt in the opposite direction so pairs never owe each other both ways, and returns both updated users sorted by name.'''

CALLS = [
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented `rest_api.py`: `GET /users` (all, or filtered by payload, sorted by name), `POST /add`, and `POST /iou` with netting against opposite-direction debt. Verified with a Python scenario covering creation, listings, both netting directions, balance signs and seeded databases (all checks passed).",
            "unverified": "",
        },
    },
]
