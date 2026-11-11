import assert from 'node:assert/strict';
import { test } from 'node:test';

import { TtlCache } from '../src/index.js';

// A clock of our own, so that the cache cannot get the time from anywhere else.
function makeClock(start = 5_000_000) {
  let now = start;
  const clock = () => now;
  clock.advance = (ms) => (now += ms);
  return clock;
}

function make(options = {}) {
  const clock = makeClock();
  const log = [];
  const cache = new TtlCache({ capacity: 3, clock, onEvict: (key, value, reason) => log.push(`${key}=${value}:${reason}`), ...options });
  return { cache, clock, log };
}

test('construction is validated', () => {
  for (const capacity of [0, -1, 1.5, '3', undefined, NaN, Infinity]) {
    assert.throws(() => new TtlCache({ capacity }), RangeError, `capacity ${capacity}`);
  }
  for (const ttl of [-1, NaN, Infinity, '100', null]) {
    assert.throws(() => new TtlCache({ capacity: 1, ttl }), RangeError, `ttl ${ttl}`);
  }
  assert.equal(new TtlCache({ capacity: 1 }).size, 0);
});

test('the ttl given to set and touch is validated too', () => {
  const { cache } = make();
  assert.throws(() => cache.set('a', 1, { ttl: -5 }), RangeError);
  assert.throws(() => cache.set('a', 1, { ttl: NaN }), RangeError);
  cache.set('a', 1);
  assert.throws(() => cache.touch('a', { ttl: Infinity }), RangeError);
});

test('alive before t + n, expired from t + n on', () => {
  const { cache, clock } = make({ ttl: 100 });
  cache.set('a', 1);
  clock.advance(99);
  assert.equal(cache.has('a'), true);
  assert.equal(cache.peek('a'), 1);
  clock.advance(1);
  assert.equal(cache.has('a'), false);
  assert.equal(cache.peek('a'), undefined);
  assert.equal(cache.get('a'), undefined);
});

test('only the injected clock is used', () => {
  const { cache, clock } = make({ ttl: 1_000 });
  cache.set('a', 1);
  clock.advance(999);
  assert.equal(cache.get('a'), 1);
  clock.advance(1);
  assert.equal(cache.get('a'), undefined);
  assert.equal(cache.size, 0);
});

test('reading does not prolong life', () => {
  const { cache, clock } = make({ ttl: 100 });
  cache.set('a', 1);
  clock.advance(60);
  assert.equal(cache.get('a'), 1);
  assert.equal(cache.peek('a'), 1);
  assert.equal(cache.has('a'), true);
  clock.advance(40);
  assert.equal(cache.get('a'), undefined);
});

test('ttl 0 never expires, as a default and per entry', () => {
  const { cache, clock } = make();
  cache.set('a', 1);
  clock.advance(1e12);
  assert.equal(cache.get('a'), 1);

  const short = make({ ttl: 100 });
  short.cache.set('pinned', 1, { ttl: 0 }).set('normal', 2).set('long', 3, { ttl: 250 });
  short.clock.advance(100);
  assert.deepEqual(short.cache.keys(), ['long', 'pinned']);
  short.clock.advance(150);
  assert.deepEqual(short.cache.keys(), ['pinned']);
  assert.deepEqual(short.log, ['normal=2:expired', 'long=3:expired']);
});

test('every operation starts by removing the expired, least recently used first', () => {
  const operations = {
    size: (cache) => cache.size,
    keys: (cache) => cache.keys(),
    stats: (cache) => cache.stats,
    prune: (cache) => cache.prune(),
    has: (cache) => cache.has('zzz'),
    get: (cache) => cache.get('zzz'),
    peek: (cache) => cache.peek('zzz'),
    delete: (cache) => cache.delete('zzz'),
    touch: (cache) => cache.touch('zzz'),
  };
  for (const [name, operation] of Object.entries(operations)) {
    const { cache, clock, log } = make({ ttl: 100 });
    cache.set('a', 1).set('b', 2).set('c', 3, { ttl: 1_000 });
    cache.get('a');
    clock.advance(100);
    operation(cache);
    assert.deepEqual(log, ['b=2:expired', 'a=1:expired'], name);
    operation(cache);
    assert.deepEqual(log, ['b=2:expired', 'a=1:expired'], `${name}, a second time`);
  }
});

test('an expired entry is reported once, and as expired, whatever is done to it', () => {
  const { cache, clock, log } = make({ ttl: 100 });
  cache.set('a', 1).set('b', 2);
  clock.advance(100);
  assert.equal(cache.delete('a'), false);
  assert.equal(cache.touch('b'), false);
  cache.clear();
  assert.deepEqual(log, ['a=1:expired', 'b=2:expired']);
});

test('set makes room by expiry before it evicts', () => {
  const { cache, clock, log } = make({ ttl: 100 });
  cache.set('old', 1).set('keep', 2, { ttl: 0 }).set('also', 3, { ttl: 0 });
  clock.advance(100);
  cache.set('new', 4);
  assert.deepEqual(log, ['old=1:expired']);
  assert.deepEqual(cache.keys(), ['new', 'also', 'keep']);
  cache.set('more', 5);
  assert.deepEqual(log, ['old=1:expired', 'keep=2:capacity']);
});

test('set on an existing key replaces value and expiry and evicts nothing', () => {
  const { cache, clock, log } = make({ ttl: 100 });
  cache.set('a', 1).set('b', 2).set('c', 3);
  clock.advance(50);
  cache.set('a', 10);
  assert.deepEqual(cache.keys(), ['a', 'c', 'b']);
  assert.deepEqual(log, []);
  clock.advance(50);
  assert.deepEqual(cache.keys(), ['a']);
  assert.equal(cache.get('a'), 10);
  clock.advance(49);
  cache.set('a', 11, { ttl: 10 });
  clock.advance(9);
  assert.equal(cache.peek('a'), 11);
  clock.advance(1);
  assert.equal(cache.peek('a'), undefined);
  assert.deepEqual(log, ['b=2:expired', 'c=3:expired', 'a=11:expired']);
});

test('set on an expired key is a fresh insert', () => {
  const { cache, clock, log } = make({ ttl: 100 });
  cache.set('a', 1);
  clock.advance(100);
  cache.set('a', 2);
  assert.deepEqual(log, ['a=1:expired']);
  assert.equal(cache.get('a'), 2);
});

test('get moves to the front; peek and has do not', () => {
  const { cache, log } = make();
  cache.set('a', 1).set('b', 2).set('c', 3);
  assert.equal(cache.peek('a'), 1);
  assert.equal(cache.has('a'), true);
  assert.deepEqual(cache.keys(), ['c', 'b', 'a']);
  cache.set('d', 4);
  assert.deepEqual(log, ['a=1:capacity']);
  assert.equal(cache.get('b'), 2);
  cache.set('e', 5);
  assert.deepEqual(log, ['a=1:capacity', 'c=3:capacity']);
  assert.deepEqual(cache.keys(), ['e', 'b', 'd']);
});

test('touch restarts the life and moves to the front', () => {
  const { cache, clock } = make({ ttl: 100 });
  cache.set('a', 1).set('b', 2);
  clock.advance(80);
  assert.equal(cache.touch('a'), true);
  assert.equal(cache.touch('nope'), false);
  assert.deepEqual(cache.keys(), ['a', 'b']);
  clock.advance(99);
  assert.deepEqual(cache.keys(), ['a']);
  assert.equal(cache.touch('a', { ttl: 0 }), true);
  clock.advance(1e9);
  assert.equal(cache.peek('a'), 1);
  assert.equal(cache.touch('a', { ttl: 5 }), true);
  clock.advance(5);
  assert.equal(cache.has('a'), false);
});

test('clear reports everything, least recently used first', () => {
  const { cache, log } = make();
  cache.set('a', 1).set('b', 2).set('c', 3);
  cache.get('a');
  cache.clear();
  assert.deepEqual(log, ['b=2:cleared', 'c=3:cleared', 'a=1:cleared']);
  assert.equal(cache.size, 0);
  cache.clear();
  assert.equal(log.length, 3);
});

test('prune returns how many it removed', () => {
  const { cache, clock } = make({ ttl: 100 });
  cache.set('a', 1).set('b', 2).set('c', 3, { ttl: 0 });
  assert.equal(cache.prune(), 0);
  clock.advance(100);
  assert.equal(cache.prune(), 2);
  assert.equal(cache.prune(), 0);
  assert.equal(cache.size, 1);
});

test('stats', () => {
  const { cache, clock } = make({ ttl: 100 });
  cache.set('a', 1).set('b', 2).set('c', 3).set('d', 4);
  cache.get('b');
  cache.get('a');
  cache.get('nope');
  cache.peek('b');
  cache.peek('nope');
  cache.has('nope');
  cache.delete('c');
  const before = cache.stats;
  assert.deepEqual(before, { hits: 1, misses: 2, expired: 0, evicted: 1 });
  clock.advance(100);
  assert.deepEqual(cache.stats, { hits: 1, misses: 2, expired: 2, evicted: 1 });
  assert.equal(cache.get('b'), undefined);
  assert.deepEqual(cache.stats, { hits: 1, misses: 3, expired: 2, evicted: 1 });
  assert.deepEqual(before, { hits: 1, misses: 2, expired: 0, evicted: 1 }, 'stats returns a fresh object');
});

test('onEvict runs after the removal', () => {
  const seen = [];
  const cache = new TtlCache({
    capacity: 2,
    onEvict: (key, value, reason) => seen.push([key, reason, cache.has(key), cache.peek(key)]),
  });
  cache.set('a', 1).set('b', 2).set('c', 3);
  cache.delete('b');
  assert.deepEqual(seen, [
    ['a', 'capacity', false, undefined],
    ['b', 'deleted', false, undefined],
  ]);
});

test('keys are compared the way Map compares them', () => {
  const { cache } = make();
  const object = {};
  cache.set(NaN, 'nan').set(object, 'object').set(1, 'number');
  assert.equal(cache.get(NaN), 'nan');
  assert.equal(cache.get({}), undefined);
  assert.equal(cache.get(object), 'object');
  assert.equal(cache.get('1'), undefined);
  assert.equal(cache.has(1), true);
});

test('falsy values are values', () => {
  const { cache } = make();
  cache.set('zero', 0).set('empty', '').set('no', false);
  assert.equal(cache.get('zero'), 0);
  assert.equal(cache.peek('empty'), '');
  assert.equal(cache.has('no'), true);
  assert.deepEqual(cache.stats, { hits: 1, misses: 0, expired: 0, evicted: 0 });
});

test('works without onEvict', () => {
  const clock = makeClock();
  const cache = new TtlCache({ capacity: 1, ttl: 10, clock });
  cache.set('a', 1).set('b', 2);
  assert.equal(cache.delete('b'), true);
  cache.set('c', 3);
  clock.advance(10);
  assert.equal(cache.size, 0);
  cache.clear();
});
