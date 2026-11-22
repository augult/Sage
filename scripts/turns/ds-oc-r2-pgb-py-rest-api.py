THOUGHT = r'''The seeded database lists each debt from both sides (owes on one user, owed_by on the
other), so my loader double-counted. I will skip an owed_by entry when the same pair already
exists from the owes pass.'''

CONTENT = r'''Fixing the seeded-database double count.'''

CALLS = [
    {"name": "edit", "arguments": {"filePath": "rest_api.py", "oldString": r'''            for other, amount in (user.get("owed_by") or {}).items():
                self._add_name(other)
                self._increase(name, other, amount)''', "newString": r'''            for other, amount in (user.get("owed_by") or {}).items():
                self._add_name(other)
                # Both sides of a debt may be listed; keep one entry per pair.
                if (name, other) in self.debt or (other, name) in self.debt:
                    continue
                self._increase(name, other, amount)'''}},
]
