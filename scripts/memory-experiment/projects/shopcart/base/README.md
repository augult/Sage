# shopcart

Cart arithmetic for the storefront, in integer minor units (cents).

- `src/cart.mjs` — lines and subtotal
- `src/discount.mjs` — coupons
- `src/tax.mjs` — tax
- `src/convert.mjs`, `src/rates.mjs` — currency conversion from the weekly table in `data/rates.json`
- `test/` — tests for the built-in Node test runner; CI runs them on every push
