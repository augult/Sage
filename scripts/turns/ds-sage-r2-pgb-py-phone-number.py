THOUGHT = "Validate characters first (letters then punctuation), then digit count, country code, and NANP area/exchange rules in the order the README documents."
CONTENT = "Implemented PhoneNumber with cleaning, NANP validation and pretty printing."
CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "phone_number.py",
            "overwrite": True,
            "content": r'''class PhoneNumber:
    def __init__(self, number):
        self.number = self._clean(number)
        self.area_code = self.number[:3]

    @staticmethod
    def _clean(number):
        for char in number:
            if char.isalpha():
                raise ValueError("letters not permitted")
            if not char.isdigit() and char not in " -().+":
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

        return digits

    def pretty(self):
        return f"({self.number[:3]}) {self.number[3:6]}-{self.number[6:]}"

    def __str__(self):
        return self.pretty()

    def __repr__(self):
        return f"PhoneNumber({self.number!r})"
''',
        },
    },
    {
        "name": "shell",
        "arguments": {
            "cmd": "python3 - <<'EOF'\nfrom phone_number import PhoneNumber\n\ncases = {\n    \"+1 (613)-995-0253\": \"6139950253\",\n    \"613-995-0253\": \"6139950253\",\n    \"1 613 995 0253\": \"6139950253\",\n    \"613.995.0253\": \"6139950253\",\n    \"(223) 456-7890\": \"2234567890\",\n    \"223.456.7890\": \"2234567890\",\n    \"223 456   7890   \": \"2234567890\",\n    \"+1 2234567890\": \"2234567890\",\n}\nfor raw, expected in cases.items():\n    phone = PhoneNumber(raw)\n    assert phone.number == expected, (raw, phone.number)\n    assert phone.area_code == expected[:3], raw\n\nassert str(PhoneNumber(\"223-456-7890\")) == \"(223) 456-7890\"\n\nerrors = {\n    \"123456789\": \"must not be fewer than 10 digits\",\n    \"1234567890123\": \"must not be greater than 11 digits\",\n    \"222345678901\": \"must not be greater than 11 digits\",\n    \"22234567890\": \"11 digits must start with 1\",\n    \"123-abc-7890\": \"letters not permitted\",\n    \"123-@:!-7890\": \"punctuations not permitted\",\n    \"(023) 456-7890\": \"area code cannot start with zero\",\n    \"(123) 456-7890\": \"area code cannot start with one\",\n    \"(223) 056-7890\": \"exchange code cannot start with zero\",\n    \"(223) 156-7890\": \"exchange code cannot start with one\",\n}\nfor raw, message in errors.items():\n    try:\n        PhoneNumber(raw)\n    except ValueError as exc:\n        assert str(exc) == message, (raw, str(exc), message)\n    else:\n        raise AssertionError(raw)\n\nprint(\"all checks ok\")\nEOF",
            "expect_exit": "0",
            "expect_has": "all checks ok",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented PhoneNumber: strips formatting, raises the documented ValueErrors (letters, punctuation, too short/long, bad country code, and invalid area/exchange leading digits), exposes .number and .area_code, and formats as (NXX) NXX-XXXX via str/pretty. Verified against the README examples and every documented error case; all checks pass.",
        },
    },
]
