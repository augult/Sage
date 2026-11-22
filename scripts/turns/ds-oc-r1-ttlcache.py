THOUGHT = r'''The stub has several contract violations: validation is missing, expiry uses Date.now instead of the injected clock, expiry boundary is wrong, peek mutates order and TTL, set on an existing key does not update expiry, no touch/prune/clear reporting/stats, and expiry is not swept at the start of every operation. I will rewrite src/cache.js around a Map with LRU-first iteration order.'''
CONTENT = "The stub deviates from the contract in validation, clock use, expiry boundary, peek semantics, replacement expiry, reporting, and the missing touch/prune/stats. Rewriting src/cache.js."
CALLS = [
    {"name": "write", "arguments": {"filePath": "src/cache.js", "content": r'''// The cache. README.md has the contract.
//
// Entries live in a Map, whose iteration order is insertion order: the first key is the least
// recently used one, and re-inserting a key makes it the most recently used.

function validateTtl(ttl) {
  if (typeof ttl !== 'number' || !Number.isFinite(ttl) || ttl < 0) {
    throw new RangeError('ttl must be a non-negative finite number');
  }
  return ttl;
}

export class TtlCache {
  constructor({ capacity, ttl = 0, clock = Date.now, onEvict } = {}) {
    if (!Number.isInteger(capacity) || capacity <= 0) {
      throw new RangeError('capacity must be a positive integer');
    }
    this._capacity = capacity;
    this._ttl = validateTtl(ttl);
    this._clock = clock;
    this._onEvict = typeof onEvict === 'function' ? onEvict : undefined;
    this._entries = new Map();
    this._stats = { hits: 0, misses: 0, expired: 0, evicted: 0 };
  }

  _report(key, entry, reason) {
    if (this._onEvict) this._onEvict(key, entry.value, reason);
  }

  // Removes every expired entry, least recently used first, and returns how many went.
  _sweep() {
    const now = this._clock();
    let removed = 0;
    for (const [key, entry] of this._entries) {
      if (now >= entry.expires) {
        this._entries.delete(key);
        this._stats.expired += 1;
        removed += 1;
        this._report(key, entry, 'expired');
      }
    }
    return removed;
  }

  _expiry(ttl) {
    const life = ttl === undefined ? this._ttl : ttl;
    return life === 0 ? Infinity : this._clock() + life;
  }

  set(key, value, { ttl } = {}) {
    if (ttl !== undefined) validateTtl(ttl);
    this._sweep();
    if (this._entries.has(key)) {
      this._entries.delete(key);
      this._entries.set(key, { value, expires: this._expiry(ttl) });
      return this;
    }
    if (this._entries.size >= this._capacity) {
      const oldest = this._entries.keys().next().value;
      const entry = this._entries.get(oldest);
      this._entries.delete(oldest);
      this._stats.evicted += 1;
      this._report(oldest, entry, 'capacity');
    }
    this._entries.set(key, { value, expires: this._expiry(ttl) });
    return this;
  }

  get(key) {
    this._sweep();
    const entry = this._entries.get(key);
    if (entry === undefined) {
      this._stats.misses += 1;
      return undefined;
    }
    this._stats.hits += 1;
    this._entries.delete(key);
    this._entries.set(key, entry);
    return entry.value;
  }

  peek(key) {
    this._sweep();
    const entry = this._entries.get(key);
    return entry === undefined ? undefined : entry.value;
  }

  has(key) {
    this._sweep();
    return this._entries.has(key);
  }

  touch(key, { ttl } = {}) {
    if (ttl !== undefined) validateTtl(ttl);
    this._sweep();
    const entry = this._entries.get(key);
    if (entry === undefined) return false;
    entry.expires = this._expiry(ttl);
    this._entries.delete(key);
    this._entries.set(key, entry);
    return true;
  }

  delete(key) {
    this._sweep();
    const entry = this._entries.get(key);
    if (entry === undefined) return false;
    this._entries.delete(key);
    this._report(key, entry, 'deleted');
    return true;
  }

  clear() {
    this._sweep();
    const removed = [...this._entries];
    this._entries.clear();
    for (const [key, entry] of removed) this._report(key, entry, 'cleared');
  }

  prune() {
    return this._sweep();
  }

  get size() {
    this._sweep();
    return this._entries.size;
  }

  keys() {
    this._sweep();
    return [...this._entries.keys()].reverse();
  }

  get stats() {
    this._sweep();
    return { ...this._stats };
  }
}
'''}},
]
