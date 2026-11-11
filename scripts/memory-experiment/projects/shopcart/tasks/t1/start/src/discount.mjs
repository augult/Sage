/** Coupons: { type: 'fixed', cents }. The result is never below zero. */
export function applyCoupon(subtotalCents, coupon) {
  if (!coupon) return subtotalCents;
  if (coupon.type === 'fixed') return Math.max(0, subtotalCents - coupon.cents);
  throw new Error(`unknown coupon type: ${coupon.type}`);
}
