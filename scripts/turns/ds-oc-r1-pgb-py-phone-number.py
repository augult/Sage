THOUGHT = r'''Check the cleaning cases from the README and each error path, plus `pretty`,
`area_code` and the slices.'''

CONTENT = r'''Testing the valid formats and every error message.'''

CALLS = [
    {"name": "bash", "arguments": {"command": r'''python3 -u - <<'PY'
from phone_number import PhoneNumber

for raw in ["+1 (613)-995-0253", "613-995-0253", "1 613 995 0253", "613.995.0253", "+1 613 995 0253"]:
    print(raw, "->", PhoneNumber(raw).number)

p = PhoneNumber("(223) 456-7890")
print(p.number, p.area_code, p.exchange_code, p.subscriber_number, p.pretty)

bad = {
    "letters": "123-abc-7890",
    "punct": "123-@:!-7890",
    "short": "123456789",
    "long": "123456789012",
    "country": "22345678901",
    "area0": "0234567890",
    "area1": "1234567890",
    "exch0": "2230567890",
    "exch1": "2231567890",
}
for name, raw in bad.items():
    try:
        PhoneNumber(raw)
        print(name, "NO ERROR")
    except ValueError as err:
        print(name, "->", err)
PY'''}},
]
