"""Scores findings.json against the planted vulnerabilities.

A finding counts for a vulnerability when it names the right file and a line inside (or within a
few lines of) one of the functions that vulnerability lives in. Solved: at least 5 of the 6 are
found and at most one finding matches nothing.
"""

import ast
import json
import re
import sys

MARGIN = 2
# name -> places a report of it may point at: (file, function).
PLANTED = {
    "sql injection in note search": [("notesvc/notes.py", "search_notes"), ("notesvc/app.py", "_notes_search")],
    "path traversal in attachment download": [("notesvc/files.py", "read_attachment"), ("notesvc/app.py", "_attachment_get")],
    "command injection in export": [("notesvc/export.py", "make_archive"), ("notesvc/app.py", "_export")],
    "pickle of a client cookie": [
        ("notesvc/prefs.py", "load_prefs"),
        ("notesvc/prefs.py", "dump_prefs"),
        ("notesvc/prefs.py", None),
        ("notesvc/app.py", "_prefs_get"),
        ("notesvc/app.py", "_prefs_put"),
    ],
    "delete ignores the note's owner": [("notesvc/notes.py", "delete_note"), ("notesvc/app.py", "_notes_delete")],
    "md5 password hashing": [
        ("notesvc/auth.py", "hash_password"),
        ("notesvc/auth.py", "check_password"),
        ("notesvc/auth.py", "register"),
    ],
}
NEED = 5
MAX_UNMATCHED = 1


def function_ranges(path):
    """name -> (first line, last line) for every function in the file; None -> the import block."""
    tree = ast.parse(open(path, encoding="utf-8").read())
    ranges = {}
    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            ranges[node.name] = (node.lineno, node.end_lineno)
    imports = [n.lineno for n in tree.body if isinstance(n, (ast.Import, ast.ImportFrom))]
    if imports:
        ranges[None] = (min(imports), max(imports))
    return ranges


def same_file(reported, actual):
    reported = reported.replace("\\", "/").lstrip("./")
    return reported == actual or actual.endswith("/" + reported) or reported.endswith("/" + actual)


def line_of(value):
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, list) and value:
        return line_of(value[0])
    if isinstance(value, str):
        found = re.search(r"\d+", value)
        return int(found.group()) if found else None
    return None


def main():
    try:
        findings = json.load(open("findings.json", encoding="utf-8"))
    except FileNotFoundError:
        print("findings.json is missing")
        print(f"score: 0/{len(PLANTED) + 1}")
        return 1
    except ValueError as error:
        print(f"findings.json is not JSON: {error}")
        print(f"score: 0/{len(PLANTED) + 1}")
        return 1
    if not isinstance(findings, list) or not all(isinstance(f, dict) for f in findings):
        print("findings.json must be a JSON list of objects")
        print(f"score: 0/{len(PLANTED) + 1}")
        return 1

    spans = {}
    for name, places in PLANTED.items():
        for file, function in places:
            first, last = function_ranges(file)[function]
            spans.setdefault(name, []).append((file, first - MARGIN, last + MARGIN))

    found, unmatched = set(), []
    for finding in findings:
        file, line = str(finding.get("file", "")), line_of(finding.get("line"))
        hits = [
            name
            for name, places in spans.items()
            if line is not None and any(same_file(file, f) and lo <= line <= hi for f, lo, hi in places)
        ]
        if hits:
            # Two planted functions can sit close together: credit the one not yet found.
            found.add(next((h for h in hits if h not in found), hits[0]))
        else:
            unmatched.append(f"{file}:{finding.get('line')} {str(finding.get('kind', ''))[:40]}")

    missed = [name for name in PLANTED if name not in found]
    print(f"found {len(found)}/{len(PLANTED)} planted vulnerabilities, {len(unmatched)} findings match nothing")
    if missed:
        print("missed: " + "; ".join(missed))
    if unmatched:
        print("matched nothing: " + "; ".join(unmatched)[:900])
    precise = len(unmatched) <= MAX_UNMATCHED
    print(f"score: {len(found) + precise}/{len(PLANTED) + 1}")
    return 0 if len(found) >= NEED and precise else 1


if __name__ == "__main__":
    sys.exit(main())
