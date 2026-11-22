THOUGHT = r'''Straightforward validation order: reject letters, then any punctuation outside digits,
spaces and ()+-., then check digit count (fewer than 10, greater than 11, 11 not starting with 1),
strip a leading country code, then validate area and exchange leading digits. `number` is the
cleaned string, `area_code` a property and `pretty()` the formatted string.'''

CONTENT = r'''I'll implement `PhoneNumber`, cleaning the digits and raising the spec's `ValueError` messages in the required order, plus `area_code` and `pretty()`, then run the full suite before finishing.'''

CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "phone_number.py",
            "overwrite": True,
            "content": r'''import re


class PhoneNumber:
    def __init__(self, number):
        self.number = self._clean(number)

    @staticmethod
    def _clean(number):
        if any(char.isalpha() for char in number):
            raise ValueError("letters not permitted")
        if any(char not in "0123456789 ()+-." for char in number):
            raise ValueError("punctuations not permitted")
        digits = re.sub(r"\D", "", number)
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
        return digits

    @property
    def area_code(self):
        return self.number[:3]

    def pretty(self):
        return f"({self.number[:3]})-{self.number[3:6]}-{self.number[6:]}"
''',
        },
    },
    {
        "name": "shell",
        "arguments": {
            "cmd": "cp /tmp/phone_number_test.py . && python3 -m unittest phone_number_test -q; rc=$?; rm -f phone_number_test.py; exit $rc",
            "expect_exit": "0",
            "expect_has": "OK",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented `PhoneNumber` in `phone_number.py`: validates letters before punctuation, enforces the 10-11 digit rules and the `1` country code, raises the exact `ValueError` messages from the spec for each invalid case, stores the cleaned 10-digit number in `.number`, exposes `.area_code` and formats `(NXX)-NXX-XXXX` via `.pretty()`.",
            "unverified": "The grader supplies its own copy of the tests; verification ran the full 21-test canonical suite locally.",
        },
    },
]
