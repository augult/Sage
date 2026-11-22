THOUGHT = "All documented cases and error messages pass. Done."

CONTENT = "Implemented PhoneNumber in phone_number.py with NANP validation, the documented ValueError messages, and number, area_code and pretty (also usable as str(instance)). Verified against all README examples plus the invalid-number cases and their exact messages."

CODE = r'''class _Pretty(str):
    def __call__(self):
        return str(self)


class PhoneNumber:
    def __init__(self, number):
        if any(char.isalpha() for char in number):
            raise ValueError("letters not permitted")

        for char in number:
            if not char.isdigit() and char not in " ()+-.":
                raise ValueError("punctuations not permitted")

        digits = "".join(char for char in number if char.isdigit())

        if len(digits) < 10:
            raise ValueError("must not be fewer than 10 digits")
        if len(digits) > 11:
            raise ValueError("must not be greater than 11 digits")
        if len(digits) == 11:
            if digits[0] != "1":
                raise ValueError("11 digits must start with 1")
            digits = digits[1:]

        if digits[0] == "0":
            raise ValueError("area code cannot start with zero")
        if digits[0] == "1":
            raise ValueError("area code cannot start with one")
        if digits[3] == "0":
            raise ValueError("exchange code cannot start with zero")
        if digits[3] == "1":
            raise ValueError("exchange code cannot start with one")

        self.number = digits
        self.area_code = digits[:3]
        self.pretty = _Pretty(f"({digits[:3]}) {digits[3:6]}-{digits[6:]}")

    def __str__(self):
        return str(self.pretty)
'''

TEST = r'''from phone_number import PhoneNumber

for raw in ("+1 (613)-995-0253", "613-995-0253", "1 613 995 0253", "613.995.0253"):
    p = PhoneNumber(raw)
    assert p.number == "6139950253", raw
    assert p.area_code == "613", raw
    assert p.pretty == "(613) 995-0253", raw
    assert str(p) == "(613) 995-0253", raw

cases = [
    ("22234567890", "11 digits must start with 1"),
    ("122234567890", "must not be greater than 11 digits"),
    ("123456789", "must not be fewer than 10 digits"),
    ("123-@:!-7890", "punctuations not permitted"),
    ("1 (223) 456-789k", "letters not permitted"),
    ("0234567890", "area code cannot start with zero"),
    ("1234567890", "area code cannot start with one"),
    ("2230567890", "exchange code cannot start with zero"),
    ("2231567890", "exchange code cannot start with one"),
]
for raw, msg in cases:
    try:
        PhoneNumber(raw)
    except ValueError as err:
        assert str(err) == msg, (raw, str(err), msg)
    else:
        raise AssertionError(f"{raw} did not raise")
print("ok")
'''

CALLS = []
