'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');

const { TTLCache } = require('./ttlcache');

// A cache on a clock the test moves by hand, recording every eviction.
function make(options = {}) {
  const clock = { time: 1000 };
  const evicted = [];
  const cache = new TTLCache({
    capacity: 3,
    ttl: 100,
    now: () => clock.time,
    onEvict: (key, value, reason) => evicted.push([key, value, reason]),
    ...options,
  });
  return { cache, clock, evicted };
}

test('rejects a bad capacity or ttl', () => {
  for (const capacity of [0, -1, 1.5, '2', undefined]) {
    assert.throws(() => new TTLCache({ capacity, ttl: 10 }), RangeError);
  }
  for (const ttl of [0, -5, undefined, NaN]) {
    assert.throws(() => new TTLCache({ capacity: 1, ttl }), RangeError);
  }
  const { cache } = make();
  assert.throws(() => cache.set('a', 1, 0), RangeError);
  assert.throws(() => cache.set('a', 1, -1), RangeError);
  assert.equal(cache.size, 0);
});

test('set returns the cache, and get, peek and has find the value', () => {
  const { cache } = make();
  assert.equal(cache.set('a', 1), cache);
  assert.equal(cache.get('a'), 1);
  assert.equal(cache.peek('a'), 1);
  assert.equal(cache.has('a'), true);
  assert.equal(cache.get('b'), undefined);
  assert.equal(cache.peek('b'), undefined);
  assert.equal(cache.has('b'), false);
  assert.equal(cache.size, 1);
});

test('an entry is live just before its time to live and expired exactly at it', () => {
  const { cache, clock } = make();
  cache.set('a', 1);
  clock.time += 99;
  assert.equal(cache.get('a'), 1);
  clock.time += 1;
  assert.equal(cache.get('a'), undefined);
  assert.equal(cache.has('a'), false);
});

test('reading does not extend the life of an entry', () => {
  const { cache, clock } = make();
  cache.set('a', 1);
  clock.time += 60;
  assert.equal(cache.get('a'), 1);
  assert.equal(cache.peek('a'), 1);
  assert.equal(cache.has('a'), true);
  clock.time += 40;
  assert.equal(cache.peek('a'), undefined);
});

test('the ttl given to set applies to that entry only', () => {
  const { cache, clock } = make();
  cache.set('short', 1, 10).set('long', 2, 500).set('plain', 3);
  clock.time += 10;
  assert.deepEqual(cache.keys(), ['long', 'plain']);
  clock.time += 90;
  assert.deepEqual(cache.keys(), ['long']);
  clock.time += 399;
  assert.equal(cache.get('long'), 2);
  clock.time += 1;
  assert.equal(cache.get('long'), undefined);
});

test('expired entries are not counted or listed, even untouched', () => {
  const { cache, clock } = make();
  cache.set('a', 1).set('b', 2, 500);
  clock.time += 100;
  assert.equal(cache.size, 1);
  assert.deepEqual(cache.keys(), ['b']);
});

test('setting an existing key replaces the value, restarts its life and counts as use', () => {
  const { cache, clock, evicted } = make();
  cache.set('a', 1).set('b', 2).set('c', 3);
  clock.time += 60;
  cache.set('a', 10);
  assert.deepEqual(cache.keys(), ['b', 'c', 'a']);
  assert.equal(cache.size, 3);
  assert.deepEqual(evicted, []);
  assert.equal(cache.stats().evictions, 0);
  clock.time += 60;
  assert.equal(cache.get('a'), 10);
  assert.equal(cache.has('b'), false);
});

test('a new key in a full cache evicts the least recently used', () => {
  const { cache, evicted } = make();
  cache.set('a', 1).set('b', 2).set('c', 3);
  assert.equal(cache.get('a'), 1);
  cache.set('d', 4);
  assert.deepEqual(cache.keys(), ['c', 'a', 'd']);
  assert.deepEqual(evicted, [['b', 2, 'capacity']]);
  assert.equal(cache.stats().evictions, 1);
});

test('has and peek do not count as use', () => {
  const { cache } = make();
  cache.set('a', 1).set('b', 2).set('c', 3);
  cache.has('a');
  cache.peek('a');
  assert.deepEqual(cache.keys(), ['a', 'b', 'c']);
  cache.set('d', 4);
  assert.equal(cache.has('a'), false);
});

test('an expired entry goes before a live one', () => {
  const { cache, clock, evicted } = make({ capacity: 2 });
  cache.set('short', 1, 10).set('long', 2);
  assert.equal(cache.get('short'), 1);
  clock.time += 50;
  cache.set('new', 3);
  assert.deepEqual(cache.keys(), ['long', 'new']);
  assert.deepEqual(evicted, [['short', 1, 'expired']]);
  assert.equal(cache.stats().evictions, 0);
});

test('delete says whether a live entry was removed, and tells nobody', () => {
  const { cache, clock, evicted } = make();
  cache.set('a', 1).set('b', 2, 500);
  assert.equal(cache.delete('missing'), false);
  assert.equal(cache.delete('b'), true);
  assert.equal(cache.has('b'), false);
  assert.deepEqual(evicted, []);
  clock.time += 100;
  assert.equal(cache.delete('a'), false);
});

test('stats count gets only, and are a snapshot', () => {
  const { cache, clock } = make();
  cache.set('a', 1);
  const before = cache.stats();
  assert.deepEqual(before, { hits: 0, misses: 0, evictions: 0 });
  cache.get('a');
  cache.get('nope');
  cache.has('a');
  cache.peek('a');
  clock.time += 100;
  cache.get('a');
  assert.deepEqual(cache.stats(), { hits: 1, misses: 2, evictions: 0 });
  assert.deepEqual(before, { hits: 0, misses: 0, evictions: 0 });
});
