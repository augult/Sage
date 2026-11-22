THOUGHT = "Normalize common NANP formatting while distinguishing letters from punctuation, then validate digit count, country prefix, and NXX constraints in that order."
CONTENT = ""
CALLS = [
    {"name": "shell", "arguments": {
        "cmd": "python3 - <<'PY'\nfrom phone_number import PhoneNumber\nfor value in ('+1 (613)-995-0253', '613-995-0253', '1 613 995 0253', '613.995.0253'):\n    assert PhoneNumber(value).number == '6139950253'\nchecks = [('123456789', 'fewer than 10'), ('212345678901', 'greater than 11'), ('26134567890', '11 digits must start'), ('6130150253', 'exchange code cannot start with zero'), ('6131150253', 'exchange code cannot start with one'), ('0139950253', 'area code cannot start with zero'), ('1139950253', 'area code cannot start with one'), ('613ABC0253', 'letters not permitted')]\nfor value, message in checks:\n    try:\n        PhoneNumber(value)\n    except ValueError as error:\n        assert message in str(error), (value, error)\n    else:\n        raise AssertionError(value)\nprint('phone number checks passed')\nPY",
        "expect_exit": "0", "expect_has": "phone number checks passed"
    }},
    {"name": "finish", "arguments": {
        "summary": "Implemented NANP number normalization and the required ValueError checks for count, country prefix, area/exchange codes, letters, and invalid punctuation. Examples and validation checks passed.",
        "unverified": "The exercise grader's full test suite was not available."
    }}
]
