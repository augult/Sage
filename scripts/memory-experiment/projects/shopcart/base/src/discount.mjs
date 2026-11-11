/**
 * Coupons: { type: 'fixed', cents } or { type: 'percent', percent, maxCents? }.
 * A percent discount is rounded to the nearest cent and capped by maxCents; the result is never below zero.
 */
export function applyCoupon(subtotalCents, coupon) {
  if (!coupon) return subtotalCents;
  if (coupon.type === 'fixed') return Math.max(0, subtotalCents - coupon.cents);
  if (coupon.type === 'percent') {
    let off = Math.round((subtotalCents * coupon.percent) / 100);
    if (coupon.maxCents != null) off = Math.min(off, coupon.maxCents);
    return Math.max(0, subtotalCents - off);
  }
  throw new Error(`unknown coupon type: ${coupon.type}`);
}
