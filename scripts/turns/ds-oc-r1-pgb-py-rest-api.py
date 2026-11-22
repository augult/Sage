THOUGHT = r'''The stub is empty. I will keep debt as a pairwise signed ledger so a loan, an
opposite loan and an overpayment all fall out of one addition: `net[a][b]` is what
`a` is owed by `b`, so crediting the lender and mirroring the borrower nets the
debt, flips direction on overpayment and clears it on exact payoff. Users are
serialised sorted by name with `balance` derived from the ledger.'''

CONTENT = r'''Implementing the API now, then exercising it with a scenario script.'''

CALLS = [
    {
        "name": "write",
        "arguments": {
            "filePath": r'''rest_api.py''',
            "content": r'''import json


class RestAPI:
    def __init__(self, database=None):
        self.net = {}
        for user in (database or {}).get("users", []):
            self.net[user["name"]] = {}
            for other, amount in (user.get("owes") or {}).items():
                self._adjust(user["name"], other, -amount)
            for other, amount in (user.get("owed_by") or {}).items():
                self._adjust(user["name"], other, amount)

    def get(self, url, payload=None):
        if url != "/users":
            raise ValueError(f"unsupported GET {url}")
        wanted = None
        if payload and payload.get("users") is not None:
            wanted = set(payload["users"])
        names = sorted(n for n in self.net if wanted is None or n in wanted)
        return json.dumps({"users": [self._user(n) for n in names]})

    def post(self, url, payload=None):
        payload = payload or {}
        if url == "/add":
            name = payload["user"]
            self.net.setdefault(name, {})
            return json.dumps(self._user(name))
        if url == "/iou":
            lender = payload["lender"]
            borrower = payload["borrower"]
            amount = float(payload["amount"])
            self._adjust(lender, borrower, amount)
            names = sorted({lender, borrower})
            return json.dumps({"users": [self._user(n) for n in names]})
        raise ValueError(f"unsupported POST {url}")

    def _adjust(self, creditor, debtor, amount):
        self.net.setdefault(creditor, {})
        self.net.setdefault(debtor, {})
        value = round(self.net[creditor].get(debtor, 0.0) + amount, 2)
        if value:
            self.net[creditor][debtor] = value
            self.net[debtor][creditor] = -value
        else:
            self.net[creditor].pop(debtor, None)
            self.net[debtor].pop(creditor, None)

    def _user(self, name):
        ledger = self.net.get(name, {})
        owes = {o: -v for o, v in ledger.items() if v < 0}
        owed_by = {o: v for o, v in ledger.items() if v > 0}
        return {
            "name": name,
            "owes": owes,
            "owed_by": owed_by,
            "balance": round(sum(owed_by.values()) - sum(owes.values()), 2),
        }
''',
        },
    },
    {
        "name": "bash",
        "arguments": {
            "command": r'''cat > /tmp/check_rest.py <<'EOF'
import json
from rest_api import RestAPI


def j(x):
    return json.loads(x)


def user(name, owes=None, owed_by=None):
    owes = owes or {}
    owed_by = owed_by or {}
    return {"name": name, "owes": owes, "owed_by": owed_by,
            "balance": sum(owed_by.values()) - sum(owes.values())}


# empty database
api = RestAPI({"users": []})
assert j(api.get("/users")) == {"users": []}

# create user
assert j(api.post("/add", {"user": "Adam"})) == user("Adam")
assert j(api.get("/users")) == {"users": [user("Adam")]}

j(api.post("/add", {"user": "Bob"}))
r = j(api.post("/iou", {"lender": "Adam", "borrower": "Bob", "amount": 3.0}))
assert [u["name"] for u in r["users"]] == ["Adam", "Bob"]
assert r["users"][0] == user("Adam", owed_by={"Bob": 3.0})
assert r["users"][1] == user("Bob", owes={"Adam": 3.0})

# both directions net out
j(api.post("/iou", {"lender": "Bob", "borrower": "Adam", "amount": 1.0}))
r = j(api.get("/users"))
assert r["users"][0] == user("Adam", owed_by={"Bob": 2.0}), r
assert r["users"][1] == user("Bob", owes={"Adam": 2.0}), r

# overpayment flips the direction
r = j(api.post("/iou", {"lender": "Bob", "borrower": "Adam", "amount": 5.0}))
assert r["users"][0] == user("Adam", owes={"Bob": 3.0}), r
assert r["users"][1] == user("Bob", owed_by={"Adam": 3.0}), r

# exact payoff clears both sides
r = j(api.post("/iou", {"lender": "Adam", "borrower": "Bob", "amount": 3.0}))
assert r["users"][0] == user("Adam"), r
assert r["users"][1] == user("Bob"), r

# filtered listing, sorted by name
j(api.post("/add", {"user": "Chuck"}))
r = j(api.get("/users", {"users": ["Chuck", "Adam"]}))
assert [u["name"] for u in r["users"]] == ["Adam", "Chuck"], r

# pre-seeded database
seeded = {"users": [
    {"name": "Adam", "owes": {"Bob": 3.0}, "owed_by": {}, "balance": -3.0},
    {"name": "Bob", "owes": {}, "owed_by": {"Adam": 3.0}, "balance": 3.0},
]}
api2 = RestAPI(seeded)
r = j(api2.get("/users"))
assert r["users"][0] == user("Adam", owes={"Bob": 3.0}), r
assert r["users"][1] == user("Bob", owed_by={"Adam": 3.0}), r

# multi-party balances
api3 = RestAPI({"users": []})
for n in ("Adam", "Bob", "Chuck", "Dan"):
    api3.post("/add", {"user": n})
api3.post("/iou", {"lender": "Adam", "borrower": "Bob", "amount": 12.0})
api3.post("/iou", {"lender": "Adam", "borrower": "Chuck", "amount": 4.0})
api3.post("/iou", {"lender": "Bob", "borrower": "Adam", "amount": 5.5})
r = j(api3.get("/users"))
adam = r["users"][0]
assert adam["owes"] == {} and adam["owed_by"] == {"Bob": 6.5, "Chuck": 4.0}, adam
assert adam["balance"] == 10.5, adam

print("all checks passed")
EOF
python3 /tmp/check_rest.py; rc=$?; rm -f /tmp/check_rest.py; exit $rc''',
        },
    },
]
