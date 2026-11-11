import '../support/pin-rates.mjs';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { applyCoupon } = await import('../src/discount.mjs');

test('no coupon', () => {
  assert.equal(applyCoupon(2500, null), 2500);
});

test('fixed coupon', () => {
  assert.equal(applyCoupon(2500, { type: 'fixed', cents: 500 }), 2000);
  assert.equal(applyCoupon(300, { type: 'fixed', cents: 500 }), 0);
});

test('unknown coupon type', () => {
  assert.throws(() => applyCoupon(2500, { type: 'bogof' }), /unknown coupon type/);
});
