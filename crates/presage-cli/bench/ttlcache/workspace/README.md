# ttl-cache

An in-memory cache with a capacity (least recently used entries go first) and a time to live per
entry. No dependencies; Node 20 or later.

```js
import { TtlCache } from './src/index.js';

const cache = new TtlCache({ capacity: 500, ttl: 60_000, onEvict: (key, value, reason) => {} });
cache.set('user:7', user);
cache.set('token:7', token, { ttl: 5_000 });
cache.get('user:7');
```

    npm test        # node --test

## Contract

### Construction

`new TtlCache({ capacity, ttl = 0, clock = Date.now, onEvict })`

- `capacity`: how many entries the cache holds; a positive integer, otherwise `RangeError`.
- `ttl`: the default time to live in milliseconds; `0` means entries do not expire. A negative
  number or something that is not a finite number is a `RangeError`.
- `clock`: a function returning the current time in milliseconds. The cache never reads the time
  any other way.
- `onEvict(key, value, reason)`: called once for every entry that leaves the cache; see below.

### Expiry

An entry set at time `t` with a time to live of `n` ms is alive while the clock is before
`t + n` and expired from `t + n` on. Reading an entry does not prolong its life.

**Every public operation first removes all expired entries**, reporting each to `onEvict` with
the reason `'expired'`, the least recently used first. Whatever is described below happens
after that and only ever sees entries that are alive. (So an expired entry is never returned,
never counted, and reported exactly once.)

### Operations

- `set(key, value, { ttl } = {})` stores the entry, makes it the most recently used and returns
  the cache. Its time to live is `ttl` if given (`0`: this entry never expires, whatever the
  default), else the default; it is validated like the default. If the key is already there, its
  value and its expiry are replaced, nothing is evicted and `onEvict` is not called. Otherwise,
  if the cache is full, the least recently used entry is evicted first with the reason
  `'capacity'`.
- `get(key)` returns the value or `undefined`, and makes the entry the most recently used.
- `peek(key)` returns the value or `undefined` without touching the order.
- `has(key)` says whether the key is there, without touching the order.
- `touch(key, { ttl } = {})` restarts the entry's time to live from now (with `ttl` if given,
  else the default) and makes it the most recently used. Returns whether the key was there.
- `delete(key)` removes the entry, reason `'deleted'`. Returns whether the key was there.
- `clear()` removes everything, reason `'cleared'`, the least recently used first.
- `prune()` does nothing beyond the removal of expired entries that every operation starts with,
  and returns how many entries that removed.
- `size` (a getter) is the number of entries.
- `keys()` returns an array of the keys, the most recently used first.
- `stats` (a getter) returns a fresh object `{ hits, misses, expired, evicted }`: `get` calls that
  found and did not find their key, entries removed because they expired, and entries removed
  for the reason `'capacity'`. `peek` and `has` count as neither hit nor miss.

Keys are compared the way `Map` compares them.

### `onEvict`

Called after the cache has finished the removal, with the entry's key and value and one of the
reasons `'expired'`, `'capacity'`, `'deleted'`, `'cleared'`. Replacing a value with `set` is not
a removal.
