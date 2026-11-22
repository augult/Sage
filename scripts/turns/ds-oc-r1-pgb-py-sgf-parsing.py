THOUGHT = r'''All documented cases pass: single and multiple properties, repeated values,
empty values, node sequences, variations at several depths, the text escaping
rules, and each documented error message.'''

CONTENT = r'''Implemented `parse` in `sgf_parsing.py` as a recursive-descent parser behind the stub's `SgfTree` interface: `(` nodes `)`, each node `;` + properties + optional variation trees, each property an uppercase key with one or more `[...]` values, and sequences linking each node to the next with its variations after it.

Text values follow SGF rules: `\` plus newline removes both, escaping other whitespace gives a space, escaping anything else keeps it literally, plain newlines are preserved and other whitespace becomes spaces. Invalid input raises `ValueError` with the documented messages (`tree missing`, `tree with no nodes`, `properties without delimiter`, `property must be in uppercase`).

Verified with a scenario script covering single and multi-property trees, repeated and empty values, node chains, nested variations, the escaping cases and every error message (all checks passed).'''

CALLS = []
