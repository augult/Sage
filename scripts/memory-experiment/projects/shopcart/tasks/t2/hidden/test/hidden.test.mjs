import '../support/pin-rates.mjs';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { taxFor } = await import('../src/tax.mjs');

test('rounded once over the whole cart', () => {
  assert.equal(taxFor([{ unitCents: 105, qty: 1 }, { unitCents: 105, qty: 1 }], 7), 15);
  assert.equal(taxFor([{ unitCents: 333, qty: 1 }, { unitCents: 333, qty: 1 }, { unitCents: 333, qty: 1 }], 10), 100);
  assert.equal(taxFor([{ unitCents: 12, qty: 1 }, { unitCents: 12, qty: 1 }, { unitCents: 12, qty: 1 }, { unitCents: 12, qty: 1 }], 4), 2);
});

test('quantities and untaxed lines', () => {
  assert.equal(taxFor([{ unitCents: 199, qty: 3 }], 8.25), 49);
  assert.equal(taxFor([{ unitCents: 105, qty: 2 }, { unitCents: 5000, qty: 1, taxable: false }], 7), 15);
  assert.equal(taxFor([], 10), 0);
});
