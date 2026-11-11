"""A ledger of named accounts. All amounts are whole cents.

Accounts
    `open(name, overdraft=0)` creates an account with a balance of 0. `overdraft` is how far below
    zero the balance may go; it must be an int >= 0, else ValueError. The name must be a non-empty
    string, else ValueError. Opening a name that is already open raises LedgerError.

    `close(name)` removes an account, and is only allowed when its balance is exactly 0 (LedgerError
    otherwise). A closed name is unknown again: it can be opened afresh, and then starts with an
    empty history.

    Naming an account that is not open raises LedgerError, in every method.

Amounts
    An amount is an int greater than 0. Anything else (0, negatives, floats, strings, and the
    bools True and False) raises ValueError.

Moving money
    `deposit(name, cents)` and `withdraw(name, cents)` return the new balance.

    A withdrawal may take the balance down to exactly `-overdraft`, not below: beyond that it
    raises InsufficientFunds, which is a LedgerError.

    `transfer(source, target, cents)` moves the amount and returns the fee. A transfer of more than
    FEE_FREE_LIMIT cents costs the source a fee of 1% of the amount, rounded to the nearest cent
    with halves rounded up; a transfer of FEE_FREE_LIMIT or less is free. The source must be able
    to pay the amount and the fee together (InsufficientFunds otherwise). The fee leaves the
    ledger. Source and target must differ (LedgerError).

    An operation that raises changes nothing: no balance, no history.

History
    `history(name)` lists the account's entries, oldest first, as tuples
    `(kind, cents, balance_after)` with `cents` always positive. The kinds are "deposit",
    "withdraw", "transfer-out", "fee" (straight after its "transfer-out"), "transfer-in" and
    "interest". The list is the caller's own: changing it does not change the ledger.

    `statement(name, last)` is the `last` most recent entries, oldest first: all of them if there
    are fewer, none for `last=0`. A negative or non-int `last` raises ValueError.

Interest
    `apply_interest(basis_points)` pays every account with a balance above 0 interest of
    balance * basis_points / 10000, rounded to the nearest cent with halves rounded up, and
    returns the total paid. Accounts at or below 0 get nothing, and no entry is recorded for an
    account whose interest rounds to 0. `basis_points` must be an int >= 0, else ValueError.

`balance(name)` is an account's balance; `total()` is the sum of all balances.
"""

FEE_FREE_LIMIT = 10_000
FEE_PERCENT = 1


class LedgerError(Exception):
    pass


class InsufficientFunds(LedgerError):
    pass


def _half_up(numerator, denominator):
    """numerator / denominator to the nearest whole number, halves up. Both are non-negative."""
    return (2 * numerator + denominator) // (2 * denominator)


def _amount(cents):
    if isinstance(cents, bool) or not isinstance(cents, int) or cents <= 0:
        raise ValueError("an amount is a whole number of cents above 0")
    return cents


def _count(value, what):
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"{what} must be a whole number, 0 or more")
    return value


class Ledger:
    def __init__(self):
        self._balances = {}
        self._overdrafts = {}
        self._entries = {}

    def open(self, name, overdraft=0):
        if not isinstance(name, str) or not name:
            raise ValueError("an account needs a name")
        _count(overdraft, "the overdraft")
        if name in self._balances:
            raise LedgerError(f"{name!r} is already open")
        self._balances[name] = 0
        self._overdrafts[name] = overdraft
        self._entries[name] = []

    def close(self, name):
        self._known(name)
        if self._balances[name] != 0:
            raise LedgerError(f"{name!r} still has a balance")
        del self._balances[name]
        del self._overdrafts[name]

    def _known(self, name):
        if not isinstance(name, str) or name not in self._balances:
            raise LedgerError(f"no account named {name!r}")
        return name

    def _post(self, name, kind, change):
        self._balances[name] += change
        self._entries[name].append((kind, abs(change), self._balances[name]))

    def balance(self, name):
        return self._balances[self._known(name)]

    def total(self):
        return sum(self._balances.values())

    def deposit(self, name, cents):
        self._known(name)
        self._post(name, "deposit", _amount(cents))
        return self._balances[name]

    def withdraw(self, name, cents):
        self._known(name)
        _amount(cents)
        if self._balances[name] - cents < -self._overdrafts[name]:
            raise InsufficientFunds(f"{name!r} cannot pay {cents}")
        self._post(name, "withdraw", -cents)
        return self._balances[name]

    def transfer(self, source, target, cents):
        _amount(cents)
        self._known(source)
        self._known(target)
        if source == target:
            raise LedgerError("a transfer needs two different accounts")
        fee = _half_up(cents * FEE_PERCENT, 100) if cents > FEE_FREE_LIMIT else 0
        if self._balances[source] - cents - fee < -self._overdrafts[source]:
            raise InsufficientFunds(f"{source!r} cannot pay {cents} and a fee of {fee}")
        self._post(source, "transfer-out", -cents)
        if fee:
            self._post(source, "fee", -fee)
        self._post(target, "transfer-in", cents)
        return fee

    def history(self, name):
        return list(self._entries[self._known(name)])

    def statement(self, name, last):
        self._known(name)
        _count(last, "last")
        if last == 0:
            return []
        return list(self._entries[name][-last:])

    def apply_interest(self, basis_points):
        _count(basis_points, "the rate")
        paid = 0
        for name, balance in self._balances.items():
            if balance <= 0:
                continue
            interest = _half_up(balance * basis_points, 10_000)
            if interest:
                self._post(name, "interest", interest)
                paid += interest
        return paid
