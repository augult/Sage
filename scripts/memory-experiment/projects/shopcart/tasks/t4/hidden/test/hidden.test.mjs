import '../support/pin-rates.mjs';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { addItem, subtotal } = await import('../src/cart.mjs');

test('the same sku is merged into one line', () => {
  const cart = [];
  addItem(cart, { sku: 'mug', unitCents: 900 });
  addItem(cart, { sku: 'mug', unitCents: 900 });
  assert.deepEqual(cart, [{ sku: 'mug', unitCents: 900, qty: 2 }]);
});

test('quantities add up, a missing qty counts as 1', () => {
  const cart = [];
  addItem(cart, { sku: 'pen', unitCents: 150, qty: 3 });
  addItem(cart, { sku: 'mug', unitCents: 900 });
  addItem(cart, { sku: 'pen', unitCents: 150 });
  addItem(cart, { sku: 'pen', unitCents: 150, qty: 2 });
  assert.deepEqual(cart, [
    { sku: 'pen', unitCents: 150, qty: 6 },
    { sku: 'mug', unitCents: 900, qty: 1 },
  ]);
  assert.equal(subtotal(cart), 1800);
});

test('addItem returns the cart', () => {
  const cart = [];
  assert.equal(addItem(cart, { sku: 'tea', unitCents: 500, currency: 'EUR' }), cart);
  assert.equal(subtotal(cart), 1000);
});
