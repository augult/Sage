// The cache. README.md has the contract.
//
// Entries live in a Map, whose iteration order is insertion order: the first key is the least
// recently used one, and re-inserting a key makes it the most recently used.

function checkTtl(ttl) {
  if (typeof ttl !== 'number' || !Number.isFinite(ttl) || ttl < 0) {
    throw new RangeError('ttl must be a finite number of milliseconds, 0 or more');
  }
  return ttl;
}

export class TtlCache {
  constructor({ capacity, ttl = 0, clock = Date.now, onEvict } = {}) {
    if (!Number.isInteger(capacity) || capacity < 1) {
      throw new RangeError('capacity must be a positive integer');
    }
    this._capacity = capacity;
    this._ttl = checkTtl(ttl);
    this._clock = clock;
    this._onEvict = onEvict;
    this._entries = new Map();
    this._stats = { hits: 0, misses: 0, expired: 0, evicted: 0 };
  }

  _expiry(ttl) {
    const life = ttl === undefined ? this._ttl : checkTtl(ttl);
    return life === 0 ? Infinity : this._clock() + life;
  }

  // Removes the given [key, entry] pairs first and reports them afterwards, so that `onEvict`
  // sees a consistent cache whatever it does with it.
  _remove(pairs, reason) {
    for (const [key] of pairs) this._entries.delete(key);
    if (this._onEvict) {
      for (const [key, entry] of pairs) this._onEvict(key, entry.value, reason);
    }
    return pairs.length;
  }

  _sweep() {
    const now = this._clock();
    const expired = [...this._entries].filter(([, entry]) => now >= entry.expires);
    this._stats.expired += expired.length;
    return this._remove(expired, 'expired');
  }

  set(key, value, { ttl } = {}) {
    const expires = this._expiry(ttl);
    this._sweep();
    if (!this._entries.has(key) && this._entries.size >= this._capacity) {
      this._stats.evicted += 1;
      this._remove([this._entries.entries().next().value], 'capacity');
    }
    this._entries.delete(key);
    this._entries.set(key, { value, expires });
    return this;
  }

  get(key) {
    this._sweep();
    const entry = this._entries.get(key);
    if (!entry) {
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
    return this._entries.get(key)?.value;
  }

  has(key) {
    this._sweep();
    return this._entries.has(key);
  }

  touch(key, { ttl } = {}) {
    const expires = this._expiry(ttl);
    this._sweep();
    const entry = this._entries.get(key);
    if (!entry) return false;
    entry.expires = expires;
    this._entries.delete(key);
    this._entries.set(key, entry);
    return true;
  }

  delete(key) {
    this._sweep();
    const entry = this._entries.get(key);
    if (!entry) return false;
    this._remove([[key, entry]], 'deleted');
    return true;
  }

  clear() {
    this._sweep();
    this._remove([...this._entries], 'cleared');
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
