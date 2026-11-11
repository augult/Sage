"""Scores test_ledger.py: it must pass on ledger.py and fail on broken copies of it.

Each mutant is ledger.py with one documented rule broken. Solved: the suite passes on the real
module and fails on at least NEED of the mutants.
"""

import shutil
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

MODULE = "ledger.py"
TESTS = "test_ledger"
NEED = 11

# name -> [(text in the module, what it becomes)]
MUTANTS = {
    "overdraft limit itself refused": [
        ("if self._balances[name] - cents < -self._overdrafts[name]:", "if self._balances[name] - cents <= -self._overdrafts[name]:")
    ],
    "fee charged at exactly the limit": [("if cents > FEE_FREE_LIMIT else 0", "if cents >= FEE_FREE_LIMIT else 0")],
    "fee rounded down": [("fee = _half_up(cents * FEE_PERCENT, 100)", "fee = (cents * FEE_PERCENT) // 100")],
    "fee not counted when checking funds": [("- cents - fee < -self._overdrafts[source]", "- cents < -self._overdrafts[source]")],
    "transfer to the same account allowed": [("if source == target:", "if source == target and False:")],
    "zero accepted as an amount": [("not isinstance(cents, int) or cents <= 0:", "not isinstance(cents, int) or cents < 0:")],
    "bools accepted as amounts": [("if isinstance(cents, bool) or not isinstance(cents, int)", "if not isinstance(cents, int)")],
    "history hands out the ledger's own list": [
        ("return list(self._entries[self._known(name)])", "return self._entries[self._known(name)]")
    ],
    "statement(last=0) returns everything": [("if last == 0:", "if last == 0 and False:")],
    "InsufficientFunds is not a LedgerError": [("class InsufficientFunds(LedgerError):", "class InsufficientFunds(Exception):")],
    "overdrawn account can be closed": [("if self._balances[name] != 0:", "if self._balances[name] > 0:")],
    "reopened account keeps its old history": [("self._entries[name] = []", "self._entries.setdefault(name, [])")],
    "interest rounded down": [("interest = _half_up(balance * basis_points, 10_000)", "interest = balance * basis_points // 10_000")],
    "interest charged on overdrawn accounts": [("if balance <= 0:", "if balance == 0:")],
}


def run_tests(folder):
    """True when the suite passes there, False when it fails; None when it cannot run."""
    try:
        result = subprocess.run(
            [sys.executable, "-m", "unittest", TESTS], cwd=folder, capture_output=True, text=True, timeout=60
        )
    except subprocess.TimeoutExpired:
        return False, "timed out"
    return result.returncode == 0, result.stderr


def survives(item):
    name, edits = item
    source = Path(MODULE).read_text(encoding="utf-8")
    for old, new in edits:
        if source.count(old) != 1:
            raise SystemExit(f"mutant {name!r} does not apply")
        source = source.replace(old, new)
    with tempfile.TemporaryDirectory() as folder:
        for path in Path(".").iterdir():
            if path.is_file() and path.suffix == ".py" and path.name not in (MODULE, "verify_mutants.py"):
                shutil.copy(path, folder)
        Path(folder, MODULE).write_text(source, encoding="utf-8")
        return run_tests(folder)[0]


def main():
    total = len(MUTANTS)
    if not Path(TESTS + ".py").is_file():
        print(f"{TESTS}.py is missing")
        print(f"score: 0/{total}")
        return 1
    passed, output = run_tests(".")
    if not passed:
        print("\n".join(output.splitlines()[-8:]))
        print(f"{TESTS}.py does not pass on the real ledger.py")
        print(f"score: 0/{total}")
        return 1
    with ThreadPoolExecutor(max_workers=4) as pool:
        survived = [name for (name, _), alive in zip(MUTANTS.items(), pool.map(survives, MUTANTS.items())) if alive]
    killed = total - len(survived)
    print(f"the suite catches {killed} of {total} broken ledgers (need {NEED})")
    if survived:
        print("not caught: " + "; ".join(survived))
    print(f"score: {killed}/{total}")
    return 0 if killed >= NEED else 1


if __name__ == "__main__":
    sys.exit(main())
