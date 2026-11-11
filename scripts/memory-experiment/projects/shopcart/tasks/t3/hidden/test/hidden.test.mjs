import '../support/pin-rates.mjs';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { convert } = await import('../src/convert.mjs');

test('into a zero-decimal currency', () => {
  assert.equal(convert(1000, 'USD', 'JPY'), 1000);
  assert.equal(convert(1999, 'USD', 'KRW'), 19990);
});

test('out of a zero-decimal currency', () => {
  assert.equal(convert(500, 'JPY', 'USD'), 500);
  assert.equal(convert(55, 'JPY', 'USD'), 55);
});

test('between zero-decimal currencies', () => {
  assert.equal(convert(250, 'JPY', 'KRW'), 2500);
  assert.equal(convert(77, 'JPY', 'JPY'), 77);
});

test('currencies with cents are unchanged', () => {
  assert.equal(convert(1000, 'USD', 'EUR'), 500);
  assert.equal(convert(333, 'EUR', 'USD'), 666);
});
