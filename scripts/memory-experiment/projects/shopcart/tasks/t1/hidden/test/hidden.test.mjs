import '../support/pin-rates.mjs';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { applyCoupon } = await import('../src/discount.mjs');

test('percent coupon', () => {
  assert.equal(applyCoupon(2000, { type: 'percent', percent: 15 }), 1700);
  assert.equal(applyCoupon(1999, { type: 'percent', percent: 15 }), 1699);
});

test('maxCents caps the discount', () => {
  assert.equal(applyCoupon(10000, { type: 'percent', percent: 50, maxCents: 2000 }), 8000);
  assert.equal(applyCoupon(1000, { type: 'percent', percent: 10, maxCents: 2000 }), 900);
});

test('never below zero', () => {
  assert.equal(applyCoupon(1234, { type: 'percent', percent: 100 }), 0);
  assert.equal(applyCoupon(1234, { type: 'percent', percent: 150 }), 0);
});

test('the other types still work', () => {
  assert.equal(applyCoupon(2500, { type: 'fixed', cents: 500 }), 2000);
  assert.equal(applyCoupon(2500, undefined), 2500);
  assert.throws(() => applyCoupon(2500, { type: 'bogof' }), /unknown coupon type/);
});
