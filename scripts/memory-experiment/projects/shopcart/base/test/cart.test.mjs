import '../support/pin-rates.mjs';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { addItem, subtotal } = await import('../src/cart.mjs');

test('addItem appends a line, quantity defaults to 1', () => {
  const cart = [];
  addItem(cart, { sku: 'mug', unitCents: 900 });
  addItem(cart, { sku: 'pen', unitCents: 150, qty: 4 });
  assert.deepEqual(cart, [
    { sku: 'mug', unitCents: 900, qty: 1 },
    { sku: 'pen', unitCents: 150, qty: 4 },
  ]);
});

test('subtotal converts every line to the requested currency', () => {
  const cart = [
    { sku: 'mug', unitCents: 900, qty: 2 },
    { sku: 'tea', unitCents: 500, qty: 1, currency: 'EUR' },
  ];
  assert.equal(subtotal(cart), 2800);
  assert.equal(subtotal(cart, 'EUR'), 1400);
});
