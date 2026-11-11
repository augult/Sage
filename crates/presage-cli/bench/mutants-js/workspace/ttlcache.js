'use strict';

/**
 * A cache with a capacity and a time to live.
 *
 *   new TTLCache({ capacity, ttl, now, onEvict })
 *
 * capacity  How many entries the cache holds: a whole number, 1 or more (RangeError otherwise).
 * ttl       How long an entry lives, in milliseconds: a number above 0 (RangeError otherwise).
 * now       Returns the current time in milliseconds. Defaults to Date.now.
 * onEvict   Optional. Called as onEvict(key, value, reason) when the cache itself drops an entry:
 *           reason is 'expired' or 'capacity'. Not called for delete() or when set() replaces a value.
 *
 * Expiry
 *   An entry set at time t with a time to live of d is live while now() < t + d, and expired from
 *   t + d on. An expired entry behaves as if it were not there, whether or not anything has touched
 *   it since: it is not counted, not listed and not returned.
 *   Reading an entry never extends its life.
 *
 * Recency
 *   Entries are ordered by when they were last used. set() and a get() that finds the entry count
 *   as use; has() and peek() do not.
 *
 * set(key, value, ttl?)
 *   Stores the value and returns the cache. `ttl` overrides the cache's ttl for this entry
 *   (RangeError unless above 0). Setting a key that is already there replaces its value, restarts
 *   its life and counts as use. Setting a new key when the cache holds `capacity` live entries
 *   first evicts the least recently used one. Expired entries never take up room: a live entry is
 *   not evicted while an expired one could go instead, and replacing a key never evicts.
 *
 * get(key)      The value, or undefined.
 * peek(key)     The value, or undefined.
 * has(key)      Whether there is a live entry.
 * delete(key)   Removes the entry. Returns whether a live entry was removed.
 * size          The number of live entries.
 * keys()        The live keys, least recently used first.
 * stats()       { hits, misses, evictions }: get() calls that found a value, get() calls that did
 *               not, and entries evicted for capacity. A snapshot: it does not change afterwards.
 */
class TTLCache {
  constructor({ capacity, ttl, now = Date.now, onEvict = () => {} } = {}) {
    if (!Number.isInteger(capacity) || capacity < 1) {
      throw new RangeError('capacity must be a whole number, 1 or more');
    }
    if (!(ttl > 0)) {
      throw new RangeError('ttl must be above 0');
    }
    this._capacity = capacity;
    this._ttl = ttl;
    this._now = now;
    this._onEvict = onEvict;
    this._entries = new Map(); // key -> { value, expires }, least recently used first
    this._stats = { hits: 0, misses: 0, evictions: 0 };
  }

  _expired(entry) {
    return this._now() >= entry.expires;
  }

  _expire(key, entry) {
    this._entries.delete(key);
    this._onEvict(key, entry.value, 'expired');
  }

  _purge() {
    for (const [key, entry] of this._entries) {
      if (this._expired(entry)) {
        this._expire(key, entry);
      }
    }
  }

  // The entry if it is live. An expired one is dropped.
  _live(key) {
    const entry = this._entries.get(key);
    if (entry === undefined) {
      return undefined;
    }
    if (this._expired(entry)) {
      this._expire(key, entry);
      return undefined;
    }
    return entry;
  }

  // Like _live, and the entry becomes the most recently used.
  _use(key) {
    const entry = this._live(key);
    if (entry !== undefined) {
      this._entries.delete(key);
      this._entries.set(key, entry);
    }
    return entry;
  }

  set(key, value, ttl = this._ttl) {
    if (!(ttl > 0)) {
      throw new RangeError('ttl must be above 0');
    }
    this._purge();
    const previous = this._entries.get(key);
    if (previous !== undefined) {
      this._entries.delete(key);
    } else if (this._entries.size >= this._capacity) {
      const [oldest, entry] = this._entries.entries().next().value;
      this._entries.delete(oldest);
      this._stats.evictions += 1;
      this._onEvict(oldest, entry.value, 'capacity');
    }
    this._entries.set(key, { value, ttl, expires: this._now() + ttl });
    return this;
  }

  get(key) {
    const entry = this._use(key);
    if (entry === undefined) {
      this._stats.misses += 1;
      return undefined;
    }
    this._stats.hits += 1;
    return entry.value;
  }

  peek(key) {
    const entry = this._live(key);
    return entry === undefined ? undefined : entry.value;
  }

  has(key) {
    return this._live(key) !== undefined;
  }

  delete(key) {
    const entry = this._live(key);
    if (entry !== undefined) {
      this._entries.delete(key);
    }
    return entry !== undefined;
  }

  get size() {
    this._purge();
    return this._entries.size;
  }

  keys() {
    this._purge();
    return [...this._entries.keys()];
  }

  stats() {
    return { ...this._stats };
  }
}

module.exports = { TTLCache };
