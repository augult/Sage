"""The tariff. Data only; the rules that use it are in the README."""

# Cents for the first 500 g and for every further 500 g of billable weight.
ZONES = {
    "A": {"first_500": 495, "per_extra_500": 110},
    "B": {"first_500": 695, "per_extra_500": 165},
    "C": {"first_500": 925, "per_extra_500": 235},
    "D": {"first_500": 1475, "per_extra_500": 385},
}

# (postal-code prefix, zone, remote)
PREFIXES = [
    ("1", "A", False),
    ("2", "A", False),
    ("3", "B", False),
    ("4", "B", False),
    ("5", "B", False),
    ("6", "C", False),
    ("7", "C", False),
    ("8", "C", False),
    ("9", "D", False),
    ("29", "B", True),
    ("684", "C", True),
    ("68", "B", False),
    ("98", "D", True),
    ("AB", "C", True),
    ("A", "B", False),
]

FREE_SHIPPING_FROM = 10000
EXPRESS_UNAVAILABLE_IN = ("D",)
REMOTE_PERCENT = 15
FRAGILE_PER_UNIT = 250
FRAGILE_CAP = 1500
OVERSIZE_FEE = 1200
OVERSIZE_SIDE_CM = 120
OVERSIZE_WEIGHT_G = 30000
