THOUGHT = r'''The failure is in my scratch check, not the API: `/iou` responses are sorted by
name, so the second user in the overpayment case is Bob, but I unpacked it as
`bob, adam`. Fixing the unpacking and rerunning the same suite, then finishing.'''

CONTENT = r'''The check's unpacking contradicted the API's documented name sort; fixed and
re-ran the suite.'''

CALLS = [
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''cat > /tmp/check_rest.py <<'EOF'
import json
import os
import sys
sys.path.insert(0, os.getcwd())
from rest_api import RestAPI

def j(x): return json.loads(x)

api = RestAPI({"users": []})
assert j(api.get("/users")) == {"users": []}
assert j(api.post("/add", {"user": "Adam"})) == {
    "name": "Adam", "owes": {}, "owed_by": {}, "balance": 0.0}
assert j(api.get("/users"))["users"][0]["name"] == "Adam"
j(api.post("/add", {"user": "Bob"}))
r = j(api.post("/iou", {"lender": "Adam", "borrower": "Bob", "amount": 3.0}))
assert [u["name"] for u in r["users"]] == ["Adam", "Bob"]
adam, bob = r["users"]
assert adam["owed_by"] == {"Bob": 3.0} and adam["balance"] == 3.0
assert bob["owes"] == {"Adam": 3.0} and bob["balance"] == -3.0
# opposite direction nets instead of double-booking
r = j(api.post("/iou", {"lender": "Bob", "borrower": "Adam", "amount": 1.0}))
adam, bob = r["users"]
assert adam["owed_by"] == {"Bob": 2.0} and adam["owes"] == {}
assert bob["owes"] == {"Adam": 2.0} and bob["owed_by"] == {}
# overpay flips the direction
r = j(api.post("/iou", {"lender": "Bob", "borrower": "Adam", "amount": 5.0}))
adam, bob = r["users"]
assert adam["owed_by"] == {} and adam["owes"] == {"Bob": 3.0}
assert adam["balance"] == -3.0 and bob["balance"] == 3.0
# exact payoff clears the pair
r = j(api.post("/iou", {"lender": "Adam", "borrower": "Bob", "amount": 3.0}))
for u in r["users"]:
    assert u["owes"] == {} and u["owed_by"] == {} and u["balance"] == 0.0
# filtered listing, sorted by name
j(api.post("/add", {"user": "Chuck"}))
r = j(api.get("/users", {"users": ["Chuck", "Adam"]}))
assert [u["name"] for u in r["users"]] == ["Adam", "Chuck"]
# pre-seeded database
seeded = {"users": [{"name": "Adam", "owes": {"Bob": 3.0}, "owed_by": {},
                     "balance": -3.0},
                    {"name": "Bob", "owes": {}, "owed_by": {"Adam": 3.0},
                     "balance": 3.0}]}
api2 = RestAPI(seeded)
r = j(api2.get("/users"))
assert r["users"][0]["owes"] == {"Bob": 3.0}
assert r["users"][1]["owed_by"] == {"Adam": 3.0}
print("all checks passed")
EOF
python3 /tmp/check_rest.py; rc=$?; rm -f /tmp/check_rest.py; exit $rc''',
            "expect_has": "all checks passed",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented `rest_api.py`: `get('/users')` returns a JSON string of all users or a name-filtered subset, sorted by name, each with `owes`, `owed_by`, and a computed `balance`; `post('/add')` creates a unique user; `post('/iou')` records a loan and nets it against debt in the opposite direction so a pair never owes each other, flipping the direction on overpayment and clearing entries on exact payoff. Verified with a Python scenario covering the empty database, creation, both netting directions, overpayment, exact payoff, filtered listings and a pre-seeded database (all checks passed).",
            "unverified": "",
        },
    },
]
