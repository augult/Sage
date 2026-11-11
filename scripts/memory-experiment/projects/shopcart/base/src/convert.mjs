import { rateFor } from './rates.mjs';

/** Currencies without a minor unit: their amounts are whole units (¥500 is 500). */
const ZERO_DECIMAL = new Set(['JPY', 'KRW']);

/** Converts an amount in the smallest unit of `from` to the smallest unit of `to`, rounded to the nearest unit. */
export function convert(amount, from, to) {
  if (from === to) return amount;
  const major = amount / (ZERO_DECIMAL.has(from) ? 1 : 100);
  return Math.round(major * rateFor(from, to) * (ZERO_DECIMAL.has(to) ? 1 : 100));
}
