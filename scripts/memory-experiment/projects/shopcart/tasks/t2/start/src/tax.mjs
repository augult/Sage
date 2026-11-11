/** Tax in cents on the taxable lines of a cart ({ unitCents, qty, taxable? }). */
export function taxFor(lines, ratePercent) {
  return lines
    .filter((l) => l.taxable !== false)
    .reduce((sum, l) => sum + Math.round((l.unitCents * l.qty * ratePercent) / 100), 0);
}
