// The cache. README.md has the contract.
//
// Entries live in a Map, whose iteration order is insertion order: the first key is the least
// recently used one, and re-inserting a key makes it the most recently used.

export class TtlCache {
  constructor({ capacity, ttl = 0, clock = Date.now, onEvict } = {}) {
    this._capacity = capacity;
    this._ttl = ttl;
    this._clock = clock;
    this._onEvict = onEvict;
    this._entries = new Map();
  }

  _expiry(ttl) {
    const life = ttl || this._ttl;
    return life ? Date.now() + life : Infinity;
  }

  _alive(key) {
    const entry = this._entries.get(key);
    if (!entry) return undefined;
    if (this._clock() > entry.expires) {
      this._entries.delete(key);
      if (this._onEvict) this._onEvict(key, entry.value, 'expired');
      return undefined;
    }
    return entry;
  }

  set(key, value, { ttl } = {}) {
    const existing = this._entries.get(key);
    if (existing) {
      existing.value = value;
      return this;
    }
    if (this._entries.size >= this._capacity) {
      const [oldest, entry] = this._entries.entries().next().value;
      this._entries.delete(oldest);
      if (this._onEvict) this._onEvict(oldest, entry.value, 'capacity');
    }
    this._entries.set(key, { value, expires: this._expiry(ttl) });
    return this;
  }

  get(key) {
    const entry = this._alive(key);
    if (!entry) return undefined;
    this._entries.delete(key);
    this._entries.set(key, { value: entry.value, expires: this._expiry() });
    return entry.value;
  }

  peek(key) {
    return this.get(key);
  }

  has(key) {
    return this._alive(key) !== undefined;
  }

  delete(key) {
    return this._entries.delete(key);
  }

  clear() {
    this._entries.clear();
  }

  get size() {
    return this._entries.size;
  }

  keys() {
    return [...this._entries.keys()];
  }
}
