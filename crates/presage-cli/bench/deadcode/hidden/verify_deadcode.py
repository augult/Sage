"""Checks a clean-up of stockr: the dead code is gone and everything reachable still works.

Solved: the visible and held-out tests pass, and at least 14 of the 17 dead things are gone.
A dead file counts as gone when it is deleted or has no code left in it.
"""

import ast
import subprocess
import sys
from pathlib import Path

PACKAGE = Path("stockr")
DEAD_FILES = ["stockr/compat.py", "stockr/xml_export.py"]
DEAD_DEFS = [
    "merge_duplicates",
    "_reindex",
    "_old_median",
    "_legacy_table",
    "summarize_v1",
    "_check_weight",
    "vacuum_all",
    "chunked",
]
UNUSED_IMPORTS = [
    ("stockr/inventory.py", "os"),
    ("stockr/stats.py", "Optional"),
    ("stockr/formatters.py", "re"),
    ("stockr/report.py", "sys"),
]
# A commented-out block is still there while any of its lines is.
COMMENTED = {
    "the commented-out load_legacy": ["# def load_legacy(", "#     sku, qty = line.split"],
    "the commented-out stale check": ['# @check("stale")', "# def _check_stale("],
    "the commented-out --xml option": ['# report.add_argument("--xml"', "# if args.xml:"],
}
NEED = 14


def parse(path):
    try:
        return ast.parse(Path(path).read_text(encoding="utf-8"))
    except (OSError, SyntaxError):
        return None


def has_code(path):
    tree = parse(path)
    if tree is None:
        return Path(path).exists()
    return any(not (isinstance(n, ast.Expr) and isinstance(n.value, ast.Constant)) for n in tree.body)


def main():
    sources = sorted(PACKAGE.rglob("*.py"))
    trees = {str(path): parse(path) for path in sources}
    defined = {
        node.name
        for tree in trees.values()
        if tree is not None
        for node in ast.walk(tree)
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef))
    }
    lines = [line.strip() for path in sources for line in path.read_text(encoding="utf-8").splitlines()]

    left = [path for path in DEAD_FILES if has_code(path)]
    left += [f"{name}()" for name in DEAD_DEFS if name in defined]
    for path, name in UNUSED_IMPORTS:
        tree = trees.get(path)
        if tree is None:
            continue
        imported = any(
            (alias.asname or alias.name.split(".")[0]) == name
            for node in ast.walk(tree)
            if isinstance(node, (ast.Import, ast.ImportFrom))
            for alias in node.names
        )
        used = any(isinstance(node, ast.Name) and node.id == name for node in ast.walk(tree))
        if imported and not used:
            left.append(f"unused import {name} in {path}")
    for label, markers in COMMENTED.items():
        if any(line.startswith(marker) for line in lines for marker in markers):
            left.append(label)

    total = len(DEAD_FILES) + len(DEAD_DEFS) + len(UNUSED_IMPORTS) + len(COMMENTED)
    gone = total - len(left)
    tests = subprocess.run(
        [sys.executable, "-m", "unittest", "discover", "-s", "tests", "-t", "."], capture_output=True, text=True, timeout=90
    )
    tests_ok = tests.returncode == 0
    if not tests_ok:
        print("\n".join((tests.stderr or tests.stdout).splitlines()[-8:]))
    print(f"tests {'pass' if tests_ok else 'FAIL'}; dead code removed: {gone}/{total} (need {NEED})")
    if left:
        print("still there: " + ", ".join(left)[:600])
    print(f"score: {gone + tests_ok}/{total + 1}")
    return 0 if tests_ok and gone >= NEED else 1


if __name__ == "__main__":
    sys.exit(main())
