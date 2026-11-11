import '../support/pin-rates.mjs';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { convert } = await import('../src/convert.mjs');

test('same currency is returned unchanged', () => {
  assert.equal(convert(1234, 'USD', 'USD'), 1234);
});

test('cents to cents', () => {
  assert.equal(convert(1000, 'USD', 'EUR'), 500);
  assert.equal(convert(333, 'EUR', 'USD'), 666);
  assert.equal(convert(25, 'USD', 'EUR'), 13);
});
