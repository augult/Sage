#!/usr/bin/env python3
"""One-off backfill of the archive mirror: `python3 scripts/backfill.py 2019 2020 ...`.

Probes the index once (no point in starting if it is down), then fetches one archive per year
with a lot of patience, because the archive host throttles.
"""

import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from fetchkit import Client, RetryError

INDEX = "https://mirror.example/index.json"
PROBE = {"timeout": 2.0}
PATIENT = {"attempts": 8, "timeout": 30.0}


def main(years):
    quiet = {"backoff": 0.0, "sleep": lambda _: None}
    probe = Client(**PROBE, **quiet)
    try:
        probe.fetch(INDEX, attempts=1)
    except RetryError as error:
        print(f"index unreachable, not starting: {error}")
        return 1
    client = Client(**PATIENT, **quiet)
    missing = []
    for year in years:
        url = f"https://mirror.example/archive/{year}.tar"
        try:
            client.fetch(url)
            print(f"fetched {url}")
        except RetryError as error:
            print(f"skipped {url}: {error}")
            missing.append(year)
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
