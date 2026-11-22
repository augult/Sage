THOUGHT = "Normalize common NANP formatting while distinguishing letters from punctuation, then validate digit count, country prefix, and NXX constraints in that order."
CONTENT = ""
CALLS = [
    {"name": "edit", "arguments": {
        "path": "phone_number.py", "range": "1xo-3ke",
        "new": "import string\n\n\nclass PhoneNumber:\n    def __init__(self, number):\n        if any(character.isalpha() for character in number):\n            raise ValueError(\"letters not permitted\")\n\n        allowed = set(\" +-().\")\n        digits = []\n        for character in number:\n            if character.isdigit():\n                digits.append(character)\n            elif character not in allowed:\n                raise ValueError(\"punctuations not permitted\")\n        cleaned = \"\".join(digits)\n\n        if len(cleaned) < 10:\n            raise ValueError(\"must not be fewer than 10 digits\")\n        if len(cleaned) > 11:\n            raise ValueError(\"must not be greater than 11 digits\")\n        if len(cleaned) == 11:\n            if cleaned[0] != \"1\":\n                raise ValueError(\"11 digits must start with 1\")\n            cleaned = cleaned[1:]\n\n        if cleaned[3] == \"0\":\n            raise ValueError(\"exchange code cannot start with zero\")\n        if cleaned[3] == \"1\":\n            raise ValueError(\"exchange code cannot start with one\")\n        if cleaned[0] == \"0\":\n            raise ValueError(\"area code cannot start with zero\")\n        if cleaned[0] == \"1\":\n            raise ValueError(\"area code cannot start with one\")\n\n        self.number = cleaned\n"
    }},
    {"name": "shell", "arguments": {
        "cmd": "python3 - <<'PY'\nfrom phone_number import PhoneNumber\nfor value in ('+1 (613)-995-0253', '613-995-0253', '1 613 995 0253', '613.995.0253'):\n    assert PhoneNumber(value).number == '6139950253'\nchecks = [('123456789', 'fewer than 10'), ('212345678901', 'greater than 11'), ('26134567890', '11 digits must start'), ('6130150253', 'exchange code cannot start with zero'), ('6131150253', 'exchange code cannot start with one'), ('0139950253', 'area code cannot start with zero'), ('1139950253', 'area code cannot start with one'), ('613ABC0253', 'letters not permitted')]\nfor value, message in checks:\n    try:\n        PhoneNumber(value)\n    except ValueError as error:\n        assert message in str(error), (value, error)\n    else:\n        raise AssertionError(value)\nprint('phone number checks passed')\nPY",
        "expect_exit": "0", "expect_has": "phone number checks passed"
    }},
    {"name": "finish", "arguments": {
        "summary": "Implemented NANP number normalization and the required ValueError checks for count, country prefix, area/exchange codes, letters, and invalid punctuation. Examples and validation checks passed.",
        "unverified": "The exercise grader's full test suite was not available."
    }}
]
