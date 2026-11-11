import '../support/pin-rates.mjs';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { taxFor } = await import('../src/tax.mjs');

test('single line', () => {
  assert.equal(taxFor([{ unitCents: 1000, qty: 2 }], 8), 160);
});

test('untaxed lines are skipped', () => {
  assert.equal(taxFor([{ unitCents: 1000, qty: 1 }, { unitCents: 400, qty: 1, taxable: false }], 10), 100);
});

test('tax is computed on the cart, not line by line', () => {
  const lines = [
    { unitCents: 333, qty: 1 },
    { unitCents: 333, qty: 1 },
    { unitCents: 333, qty: 1 },
  ];
  assert.equal(taxFor(lines, 10), 100);
});
