THOUGHT = r'''All tree-shape, text-escaping and error-message checks pass. Done.'''

CONTENT = r'''`sgf_parsing.py` now has a recursive-descent `parse`: trees chain nodes and attach variations to the last node of a sequence, properties keep multiple values, and text values implement the SGF rules (escaped `]`/`\`, backslash-newline removal, other whitespace to spaces, newlines preserved). Errors raise `ValueError` with "tree missing", "tree with no nodes", "properties without delimiter", or "property must be in uppercase".

Verified with checks for single and multiple properties, multiple values, node chains, variations at several depths, all text escaping rules, and each error path (all passed).'''

CALLS = []
