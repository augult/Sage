THOUGHT = "Implementation complete and every rule verified independently of the visible tests."
CONTENT = r'''Implemented all twelve README rules in `shipping/zones.py`, `shipping/weights.py`, `shipping/surcharges.py` and `shipping/quote.py`:

- Zone lookup strips spaces and upper-cases, then takes the longest matching prefix from `rates.PREFIXES`, raising `QuoteError` when nothing matches.
- Weight maths stays integral: per-unit volume divided by 5 rounded up, and the larger of actual and volumetric total rounded up to a 500 g multiple.
- Base rate uses the zone tariff, express multiplies by 1.5 with half-up rounding and is rejected in zone D, a non-remote standard order from 10,000 cents merchandise ships free, then `SHIP50` halves the base downwards and `FREESHIP` zeroes it (no effect when remote).
- Remote 15% half up, fragile 250 per fragile unit capped at 1,500, oversize 1,200 once, with zero surcharges omitted and total = base + surcharges.
- Validation rejects no items, non-positive or non-integer quantity, weight and sides, and unknown services.

`python3 -m unittest discover -s tests` passes 6/6, and a rule-by-rule script of 34 cases covering all twelve rules passes.'''
CALLS = []
