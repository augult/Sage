# shipping

Shipping quotes for the web shop. `shipping.quote.quote(order)` takes an order and returns what
the customer pays for delivery. All money is in integer cents, all weights in integer grams.

    python3 -m unittest discover -s tests

## Status

`rates.py` (the tariff) and `errors.py` are done. `zones.py`, `weights.py`, `surcharges.py` and
`quote.py` are stubs: the rules below are the specification.

## The order

```python
{
    "destination": "68 410",          # postal code as the customer typed it
    "service": "standard",            # or "express"
    "coupon": "SHIP50",               # optional; absent or None means no coupon
    "items": [
        {
            "sku": "LAMP-01",
            "qty": 2,
            "price": 3490,            # cents per unit
            "weight_g": 1800,         # grams per unit
            "dims_cm": [40, 30, 25],  # one unit, any order of sides, whole centimetres
            "fragile": True,          # optional, default False
        },
    ],
}
```

## The quote

The shape of the result. The figures only illustrate it: they are not the quote of the order
above.

```python
{
    "zone": "C",
    "billable_g": 3000,
    "base": 2100,
    "surcharges": {"fragile": 500},   # only the surcharges that are not zero
    "total": 2600,
}
```

## Rules

Anything that makes a quote impossible raises `shipping.errors.QuoteError`.

### Zones (`zones.py`)

1. The postal code is normalised first: spaces are removed and letters upper-cased, so
   `" ab1 2cd "` is `"AB12CD"`.
2. `rates.PREFIXES` maps postal-code prefixes to a zone and a "remote" flag. The **longest**
   prefix that the code starts with decides, whatever its position in the table. A code that no
   prefix matches is an error.

### Weight (`weights.py`)

3. The volumetric weight of one unit is its volume in cm³ divided by 5, in grams, rounded **up**
   to a whole gram. A line's volumetric weight is that times `qty`.
4. The billable weight is decided for the shipment, not per line: the larger of the total actual
   weight and the total volumetric weight, rounded **up** to the next multiple of 500 g. A weight
   that is already a multiple of 500 g stays as it is.

### Base rate (`quote.py`)

5. The tariff of the zone gives the rate: `first_500` for the first 500 g plus `per_extra_500`
   for every further 500 g of billable weight.
6. Express costs one and a half times the rate, rounded half up to a whole cent. Express is not
   available in zone D: asking for it there is an error.
7. Free shipping: a standard-service order whose merchandise (the sum of `qty * price`) is at
   least 10000 cents ships with a base rate of 0, unless the destination is remote.
8. Coupons apply after rule 7. `SHIP50` halves the base rate, rounding the result **down**.
   `FREESHIP` sets it to 0 for any service, but does nothing for a remote destination. Coupon
   codes are case-sensitive; any other code is an error.

`base` in the quote is the base rate after rules 5 to 8.

### Surcharges (`surcharges.py`)

9. Remote: 15 % of `base`, rounded half up to a whole cent, for a remote destination.
10. Fragile: 250 cents for every fragile unit (a line with `qty` 3 is three units), capped at
    1500 cents per shipment.
11. Oversize: 1200 cents, once per shipment, if any unit has a side longer than 120 cm or weighs
    more than 30000 g.

`total` is `base` plus the surcharges.

### Validation (`quote.py`)

12. These are errors: no items; a `qty` that is not a positive integer; a `weight_g` or a side
    that is not a positive integer; a `service` other than `standard` or `express`.

"Rounded half up" means that exactly half a cent goes up: 1012.5 becomes 1013, and so does
1013.5 become 1014. Money never passes through a float.
