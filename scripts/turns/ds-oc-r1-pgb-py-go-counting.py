THOUGHT = r'''The checks all match the expected contract: a single-colour surround is owned,
mixed borders yield no owner, a stone coordinate reports its colour with an empty set, the
whole-board scan only includes owned regions, and every invalid coordinate raises ValueError
with a message. Done.'''

CONTENT = r'''Implemented `go_counting.py` with flood-fill territory detection. Empty regions
are grouped by connectivity (horizontal/vertical), and a region belongs to a player only when
exactly one colour borders it; mixed borders stay unowned and are omitted from `territories()`.
`territory()` on a stone returns that stone's colour with an empty set, and non-integer or
out-of-bounds coordinates raise `ValueError("Invalid coordinate")`. Self-checks on single-owner,
mixed, stone, empty-board and invalid-coordinate cases all pass.'''

CALLS = []
