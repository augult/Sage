"""A small in-process cache for note listings."""

import hashlib
import random
import time


def cache_key(*parts):
    joined = "\x1f".join(str(part) for part in parts)
    return hashlib.md5(joined.encode("utf-8")).hexdigest()


class TTLCache:
    def __init__(self, ttl=30, max_entries=512):
        self.ttl = ttl
        self.max_entries = max_entries
        self._entries = {}

    def get(self, key):
        entry = self._entries.get(key)
        if entry is None or entry[0] < time.monotonic():
            self._entries.pop(key, None)
            return None
        return entry[2]

    def put(self, key, owner_id, value):
        if len(self._entries) >= self.max_entries:
            self._entries.clear()
        # Spread the expiries so that entries filled together do not all lapse together.
        expires = time.monotonic() + self.ttl + random.uniform(0, 5)
        self._entries[key] = (expires, owner_id, value)

    def drop_owner(self, owner_id):
        self._entries = {key: entry for key, entry in self._entries.items() if entry[1] != owner_id}
