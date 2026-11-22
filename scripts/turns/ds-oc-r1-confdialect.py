THOUGHT = """All visible tests and the extra spec checks pass, so the task is complete."""

CONTENT = r'''`conf/parser.py` now implements all of SPEC.md:

- Line handling with `\r\n`, BOM, blank lines and whole-line comments.
- Section headers with strict name validation and merging of repeated headers; entries split on the first `=` with key validation.
- Plain values with blank-preceded inline comments and backslash continuations joined per §5 (including "no space for an empty piece" and the EOF backslash error).
- Quoted values with `\\`, `\"`, `\n`, `\t` and `\uXXXX` escapes, plus the trailing-comment rule.
- Lists with `name[]`, including the duplicate and mixed `name`/`name[]` errors reported on the later line.
- A reference pass after parsing: `${key}` with top-section fallback, `${section:key}`, `${:key}`, `$$`, literal `$`, forward references, reference-to-list and cycle errors.
- Error line numbers per §9, and the §10 result shape (ordered sections, top section only when non-empty, empty header-only sections kept).

Verification: `python3 -m unittest discover -s tests` passes 7/7, `python3 -m conf examples/app.conf` succeeds, and extra spec checks (lists, continuations, quoted comments, `$$`, duplicate/mixed keys, bad references, cycles) pass.'''

CALLS = []
