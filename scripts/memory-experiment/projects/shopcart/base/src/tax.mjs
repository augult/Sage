/** Tax in cents on the taxable lines of a cart ({ unitCents, qty, taxable? }). */
export function taxFor(lines, ratePercent) {
  const taxable = lines.filter((l) => l.taxable !== false).reduce((sum, l) => sum + l.unitCents * l.qty, 0);
  return Math.round((taxable * ratePercent) / 100);
}
