"""Checks report.customer_report: the reference's results, with a fixed number of queries.

Solved when all four hold: the visible tests pass; on 200 small random databases the report equals
the reference's; on a database of 3,000 customers, 15,000 orders and 60,000 items it equals the
reference's within LIMIT_FACTOR times the reference's own time (at least MIN_LIMIT seconds); and
on that database it runs at most MAX_QUERIES statements and opens the rates file once.
"""

import json
import os
import random
import sqlite3
import subprocess
import sys
import time

LIMIT_FACTOR = 10
MIN_LIMIT = 4.0
MAX_QUERIES = 10
RATES = os.path.abspath("_rates_check.json")
SCHEMA_FROM = "shop"

SKUS = ["BOLT", "NUT", "GEAR", "AXLE", "CAM", "PIN", "RING", "SHIM"]
CURRENCIES = ["EUR", "USD", "GBP", "SEK", "CHF"]
STATUSES = ["open", "paid", "shipped", "cancelled"]


def build(rng, customers, orders, items):
    import shop

    conn = shop.connect()
    shop.create(conn)
    names = [f"Customer {n:04d}" for n in range(max(1, customers * 3 // 4))]
    conn.executemany(
        "INSERT INTO customers (id, name, region) VALUES (?, ?, ?)",
        [(n + 1, rng.choice(names), rng.choice(["north", "south", "east"])) for n in range(customers)],
    )
    conn.executemany(
        "INSERT INTO orders (id, customer_id, placed_on, status, currency) VALUES (?, ?, ?, ?, ?)",
        [
            (
                n + 1,
                rng.randint(1, customers),
                f"2024-{rng.randint(1, 12):02d}-{rng.randint(1, 28):02d}",
                rng.choice(STATUSES),
                rng.choice(CURRENCIES),
            )
            for n in range(orders if customers else 0)
        ],
    )
    conn.executemany(
        "INSERT INTO items (order_id, sku, qty, unit_cents, discount_pct) VALUES (?, ?, ?, ?, ?)",
        [
            (
                rng.randint(1, orders),
                rng.choice(SKUS),
                rng.randint(1, 9),
                rng.choice([5, 15, 25, 99, 125, 333, 1250, 4999]),
                rng.choice([None, None, 0, 10, 25, 33, 50, 100]),
            )
            for _ in range(items if customers and orders else 0)
        ],
    )
    conn.commit()
    return conn


def small_databases():
    rng = random.Random(4117)
    for _ in range(200):
        customers = rng.choice([0, 1, 2, 3, 5, 8])
        orders = rng.choice([0, 1, 3, 6, 12])
        yield build(rng, customers, orders, rng.choice([0, 2, 8, 20, 40]))


def large_database():
    return build(random.Random(88), 3000, 15_000, 60_000)


def drive():
    """Run in a child process: the workspace's report on every database, results on stdout."""
    import report

    answers = [report.customer_report(conn, RATES) for conn in small_databases()]
    print(json.dumps({"small": answers}), flush=True)

    conn = large_database()
    counts = {"queries": 0, "opens": 0}

    def on_statement(_):
        counts["queries"] += 1

    def on_audit(event, args):
        if event == "open" and isinstance(args[0], (str, bytes, os.PathLike)):
            if os.path.basename(os.fsdecode(args[0])) == os.path.basename(RATES):
                counts["opens"] += 1

    sys.addaudithook(on_audit)
    conn.set_trace_callback(on_statement)
    started = time.perf_counter()
    result = report.customer_report(conn, RATES)
    elapsed = time.perf_counter() - started
    conn.set_trace_callback(None)
    print(json.dumps({"large": result, "seconds": elapsed, **counts}), flush=True)


def main():
    import _reference_report as reference

    score = 0
    tests = subprocess.run([sys.executable, "-m", "unittest", "test_report"], capture_output=True, text=True, timeout=60)
    if tests.returncode == 0:
        score += 1
        print("visible tests: pass")
    else:
        print("visible tests: FAIL")

    conn = large_database()
    timings = []
    for _ in range(2):
        started = time.perf_counter()
        expected_large = reference.customer_report(conn, RATES)
        timings.append(time.perf_counter() - started)
    limit = max(MIN_LIMIT, LIMIT_FACTOR * min(timings))

    child = subprocess.Popen([sys.executable, __file__, "drive"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        out, err = child.communicate(timeout=limit + 15)
    except subprocess.TimeoutExpired:
        child.kill()
        out, err = child.communicate()
    lines = [json.loads(line) for line in out.splitlines() if line.startswith("{")]
    small = next((line["small"] for line in lines if "small" in line), None)
    large = next((line for line in lines if "large" in line), None)

    expected_small = [reference.customer_report(conn, RATES) for conn in small_databases()]
    if small is None:
        print("random databases: the report raised or did not finish: " + (err.strip().splitlines() or ["no output"])[-1][:200])
    else:
        wrong = [index for index, (got, want) in enumerate(zip(small, expected_small)) if got != want]
        if wrong:
            got, want = small[wrong[0]], expected_small[wrong[0]]
            pair = next(((g, w) for g, w in zip(got, want) if g != w), (got[len(want):], want[len(got):]))
            print(f"random databases: {len(wrong)} of {len(small)} differ from the reference, e.g. got {pair[0]} want {pair[1]}"[:700])
        else:
            score += 1
            print(f"random databases: all {len(small)} match the reference")

    if large is None:
        print(f"large database: not finished within {limit:.1f}s (the reference takes {min(timings):.2f}s)")
    elif large["large"] != expected_large:
        print("large database: the report differs from the reference")
    elif large["seconds"] > limit:
        print(f"large database: {large['seconds']:.1f}s, over the limit of {limit:.1f}s (the reference takes {min(timings):.2f}s)")
    else:
        score += 1
        print(f"large database: {large['seconds']:.2f}s, limit {limit:.1f}s (the reference takes {min(timings):.2f}s)")

    if large is not None:
        frugal = large["queries"] <= MAX_QUERIES and large["opens"] <= 1
        score += frugal
        print(
            f"large database: {large['queries']} statements (at most {MAX_QUERIES}), "
            f"rates file opened {large['opens']} times (at most 1): {'ok' if frugal else 'TOO MANY'}"
        )

    print(f"score: {score}/4")
    return 0 if score == 4 else 1


if __name__ == "__main__":
    if sys.argv[1:] == ["drive"]:
        drive()
    else:
        sys.exit(main())
