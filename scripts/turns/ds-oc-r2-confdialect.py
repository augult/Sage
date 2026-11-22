THOUGHT = """Everything passes, so the task is complete."""

CONTENT = r'''`conf/parser.py` now implements all of SPEC.md:

- `\r\n`/BOM handling, blank lines and whole-line comments.
- Strict section headers and keys, repeated headers merged, entries split on the first `=`.
- Plain values with blank-preceded inline comments and backslash continuations (§5, including the empty-piece and end-of-file backslash rules).
- Quoted values with `\\`, `\"`, `\n`, `\t`, `\uXXXX` and the trailing-comment rule.
- Lists, with duplicate and `name`/`name[]` conflicts reported on the later line.
- A reference pass with `${key}` (top-section fallback), `${section:key}`, `${:key}`, `$$`, forward references, and errors for missing keys/sections, references to lists, unclosed references and cycles.
- §9 line numbers and the §10 result shape.

Verified with `python3 -m unittest discover -s tests` (7/7), `python3 -m conf examples/app.conf`, and extra spec checks for the rules the visible suite does not cover.'''

CALLS = []
