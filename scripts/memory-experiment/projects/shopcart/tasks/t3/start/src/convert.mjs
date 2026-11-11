import { rateFor } from './rates.mjs';

/** Converts an amount in cents of `from` to cents of `to`, rounded to the nearest cent. */
export function convert(amount, from, to) {
  if (from === to) return amount;
  return Math.round(amount * rateFor(from, to));
}
