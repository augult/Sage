import assert from 'node:assert/strict';
import { test } from 'node:test';

import { TtlCache, manualClock } from '../src/index.js';

test('stores and returns values', () => {
  const cache = new TtlCache({ capacity: 2 });
  cache.set('a', 1).set('b', 2);
  assert.equal(cache.get('a'), 1);
  assert.equal(cache.get('nope'), undefined);
  assert.equal(cache.size, 2);
});

test('evicts the least recently used entry', () => {
  const evicted = [];
  const cache = new TtlCache({ capacity: 2, onEvict: (key, value, reason) => evicted.push([key, value, reason]) });
  cache.set('a', 1).set('b', 2);
  cache.get('a');
  cache.set('c', 3);
  assert.deepEqual(cache.keys(), ['c', 'a']);
  assert.deepEqual(evicted, [['b', 2, 'capacity']]);
});

test('an entry is gone once its time is up', () => {
  const clock = manualClock(1_000);
  const cache = new TtlCache({ capacity: 5, ttl: 100, clock });
  cache.set('a', 1);
  clock.advance(99);
  assert.equal(cache.get('a'), 1);
  clock.advance(1);
  assert.equal(cache.get('a'), undefined);
});

test('expired entries are not counted', () => {
  const clock = manualClock();
  const cache = new TtlCache({ capacity: 5, ttl: 100, clock });
  cache.set('a', 1).set('b', 2, { ttl: 500 });
  clock.advance(200);
  assert.equal(cache.size, 1);
  assert.deepEqual(cache.keys(), ['b']);
});

test('delete reports the entry', () => {
  const evicted = [];
  const cache = new TtlCache({ capacity: 2, onEvict: (key, value, reason) => evicted.push([key, value, reason]) });
  cache.set('a', 1);
  assert.equal(cache.delete('a'), true);
  assert.equal(cache.delete('a'), false);
  assert.deepEqual(evicted, [['a', 1, 'deleted']]);
});
