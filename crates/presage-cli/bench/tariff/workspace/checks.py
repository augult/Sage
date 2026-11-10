"""Checks shipping_cost against the published tariff table. Prints one line per case."""
import math
import sys

from tariff import shipping_cost

RATES = {"A": 1.00, "B": 1.40, "C": 1.90, "D": 2.60, "E": 3.50}
WEIGHTS = [0.5, 1, 1.2, 2, 3.7, 5, 10, 12.5, 19.9, 20, 20.1, 25, 30, 50]


def expected(weight, zone, express):
    cost = 4.00 + math.ceil(weight) * RATES[zone]
    if weight >= 20:
        cost += 12.00
    if express:
        cost *= 1.5
    return round(cost, 2)


def main():
    failed = []
    number = 0
    for express in (False, True):
        for zone in "ABCDE":
            for weight in WEIGHTS:
                number += 1
                want = expected(weight, zone, express)
                got = shipping_cost(weight, zone, express)
                label = f"case {number:03d}: {weight:>5} kg  zone {zone}  {'express ' if express else 'standard'}"
                if got == want:
                    print(f"ok    {label}  {got:8.2f}")
                else:
                    failed.append(number)
                    print(f"FAIL  {label}  expected {want:.2f}, got {got:.2f}")
    print(f"{number - len(failed)} passed, {len(failed)} failed")
    if failed:
        print("failed cases: " + ", ".join(str(n) for n in failed))
        sys.exit(1)


if __name__ == "__main__":
    main()
