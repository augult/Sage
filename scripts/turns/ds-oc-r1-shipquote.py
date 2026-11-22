THOUGHT = "Implementation is complete, visible tests and all twelve rules verified."
CONTENT = r'''Implemented all twelve rules across `shipping/zones.py`, `shipping/weights.py`, `shipping/surcharges.py` and `shipping/quote.py`:

- Zone lookup normalises the code and picks the longest matching prefix, raising `QuoteError` when nothing matches.
- Weights use integer-only arithmetic: per-unit volume over 5 rounded up, and the shipment maximum rounded up to a 500 g multiple.
- Base rate from the zone tariff, 1.5x with half-up rounding for express, free shipping at 10,000 cents merchandise for non-remote standard orders, then `SHIP50` (half, down) and `FREESHIP` (zero, not remote) coupons applied afterwards.
- Remote 15%, fragile 250 per unit capped at 1,500, oversize 1,200, with zero surcharges omitted.
- Validation for empty items, non-positive or non-integer quantity, weight and sides, and unknown services.

`python3 -m unittest discover -s tests` passes 6/6, and a rule-by-rule check of 32 cases across all twelve rules passes.'''
CALLS = []
