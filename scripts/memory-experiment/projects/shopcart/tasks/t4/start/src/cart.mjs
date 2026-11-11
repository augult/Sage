import { convert } from './convert.mjs';

/** A cart is an array of lines: { sku, unitCents, qty, currency? } (currency defaults to USD). */
export function addItem(cart, item) {
  cart.push({ ...item, qty: item.qty ?? 1 });
  return cart;
}

/** Sum of all lines in the smallest unit of `currency`. */
export function subtotal(cart, currency = 'USD') {
  return cart.reduce((sum, l) => sum + convert(l.unitCents * l.qty, l.currency ?? 'USD', currency), 0);
}
