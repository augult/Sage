"""Held-out tariff cases: weights and combinations the visible checks do not use."""
import math
import sys

from tariff import shipping_cost

RATES = {"A": 1.00, "B": 1.40, "C": 1.90, "D": 2.60, "E": 3.50}

bad = 0
for express in (False, True):
    for zone, rate in RATES.items():
        for weight in (0.01, 0.99, 7, 7.0001, 19.99, 20.0, 20.5, 99.5):
            cost = 4.00 + math.ceil(weight) * rate
            if weight >= 20:
                cost += 12.00
            if express:
                cost *= 1.5
            want = round(cost, 2)
            got = shipping_cost(weight, zone, express)
            if got != want:
                bad += 1
                print(f"FAIL {weight} kg zone {zone} express={express}: expected {want}, got {got}")
print(f"held-out cases failed: {bad}")
sys.exit(1 if bad else 0)
