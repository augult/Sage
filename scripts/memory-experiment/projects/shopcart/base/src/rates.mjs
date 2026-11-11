import { readFileSync } from 'node:fs';

const table = JSON.parse(readFileSync(new URL('../data/rates.json', import.meta.url), 'utf8'));

/** Units of `to` per one unit of `from`, from the weekly table in data/rates.json. */
export function rateFor(from, to) {
  if (from === to) return 1;
  const rate = table[from]?.[to];
  if (rate == null) throw new Error(`no rate for ${from}>${to}`);
  return rate;
}
