"""Checks the fix and its regression test.

Solved when all three hold: the whole suite passes (the visible tests, the held-out tests of the
other paths that share the price tables, and test_regression.py); test_regression.py passes by
itself on the fixed code; and it fails by itself on the original code in _original/.
"""

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REGRESSION = "test_regression.py"


def unittest(args, cwd="."):
    try:
        result = subprocess.run([sys.executable, "-m", "unittest", *args], cwd=cwd, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired:
        return False, "timed out", 0
    ran = [line for line in result.stderr.splitlines() if line.startswith("Ran ")]
    count = int(ran[-1].split()[1]) if ran else 0
    return result.returncode == 0, result.stderr, count


def main():
    score = 0
    passed, output, _ = unittest([])
    if passed:
        score += 1
        print("whole suite: pass")
    else:
        failing = [line for line in output.splitlines() if line.startswith(("FAIL:", "ERROR:"))]
        print("whole suite: FAIL: " + "; ".join(failing)[:500])

    if not Path(REGRESSION).is_file():
        print(f"{REGRESSION} is missing")
    else:
        passed, output, count = unittest(["test_regression"])
        if passed and count:
            score += 1
            print(f"{REGRESSION} by itself on the fixed code: pass ({count} tests)")
        else:
            print(f"{REGRESSION} by itself on the fixed code: " + ("runs no tests" if passed else "FAIL"))
        with tempfile.TemporaryDirectory() as folder:
            for path in Path("_original").iterdir():
                shutil.copy(path, folder)
            shutil.copy(REGRESSION, folder)
            passed, output, count = unittest(["test_regression"], cwd=folder)
        if passed:
            print(f"{REGRESSION} by itself on the original code: passes, so it does not catch the bug")
        else:
            score += 1
            print(f"{REGRESSION} by itself on the original code: fails, as it should")

    print(f"score: {score}/3")
    return 0 if score == 3 else 1


if __name__ == "__main__":
    sys.exit(main())
