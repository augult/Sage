THOUGHT = r'''Every README example cleans to ten digits, all nine error paths raise the exact messages,
and area code/exchange/subscriber/pretty are consistent. Done.'''

CONTENT = r'''Implemented `phone_number.py` with NANP cleaning and validation. Letters are rejected
before punctuation, digit count is checked with the 11-digit country-code rule, and area/exchange
codes reject leading 0 or 1 with the specified messages. Exposes `number`, `area_code`,
`exchange_code`, `subscriber_number` and `pretty` (e.g. "(223) 456-7890"). Verified all README
inputs and every documented error path.'''

CALLS = []
