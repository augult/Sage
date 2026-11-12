"""Checks weblog.analyze: the same results as the reference, and fast on a production-sized log.

Solved when all three hold: the visible tests pass; on 300 small random logs (full of ties and
repeats) the results equal the reference's; and a log of 200,000 events from 40,000 users gives
the reference's result within LIMIT_FACTOR times the reference's own time (at least MIN_LIMIT
seconds, so that a loaded machine does not fail a good solution).
"""

import json
import random
import subprocess
import sys
import time

LIMIT_FACTOR = 10
MIN_LIMIT = 4.0


def small_logs():
    rng = random.Random(20240611)
    users = ["ana", "ben", "cy", "dee", "eli", "fay"]
    pages = ["/", "/docs", "/docs/", "/Docs?x=1", "/docs//api", "/docs/api/", "/pricing#top", "/PRICING", "/blog", "//blog/"]
    logs = []
    for case in range(300):
        size = rng.choice([0, 1, 2, 3, 5, 8, 13, 21, 40])
        log = [
            (rng.choice(users[: rng.randint(1, 6)]), rng.randrange(0, 12) * rng.choice([1, 30, 60]), rng.choice(pages))
            for _ in range(size)
        ]
        logs.append((log, rng.choice([0, 30, 60, 61, 1800])))
    return logs


def large_log():
    rng = random.Random(977)
    events = []
    for index in range(200_000):
        if rng.random() < 0.5:
            user = f"u{rng.randrange(40_000)}"
        else:
            user = f"u{int(rng.paretovariate(0.6)) % 40_000}"
        page = f"/p/{int(rng.paretovariate(0.9)) % 3000}"
        if rng.random() < 0.2:
            page += rng.choice(["/", "?ref=mail", "#top"])
        events.append((user, index // 3 + rng.randrange(-40, 40), page))
    return events


def plain(result):
    """The result as JSON-shaped data, so that tuples and lists compare equal."""
    return json.loads(json.dumps(result))


def drive():
    """Run in a child process: the workspace's analyze on every case, results on stdout."""
    import weblog

    answers = [plain(weblog.analyze(log, gap)) for log, gap in small_logs()]
    print(json.dumps({"small": answers}), flush=True)
    events = large_log()
    started = time.perf_counter()
    result = weblog.analyze(events)
    elapsed = time.perf_counter() - started
    print(json.dumps({"large": plain(result), "seconds": elapsed}), flush=True)


def main():
    import _reference_weblog as reference

    score = 0
    tests = subprocess.run([sys.executable, "-m", "unittest", "test_weblog"], capture_output=True, text=True, timeout=60)
    if tests.returncode == 0:
        score += 1
        print("visible tests: pass")
    else:
        print("visible tests: FAIL")

    events = large_log()
    timings = []
    for _ in range(2):
        started = time.perf_counter()
        expected_large = plain(reference.analyze(events))
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

    expected_small = [plain(reference.analyze(log, gap)) for log, gap in small_logs()]
    if small is None:
        print("random logs: analyze raised or did not finish: " + (err.strip().splitlines() or ["no output"])[-1][:200])
    else:
        wrong = [index for index, (got, want) in enumerate(zip(small, expected_small)) if got != want]
        if wrong:
            log, gap = small_logs()[wrong[0]]
            keys = [key for key in expected_small[wrong[0]] if small[wrong[0]].get(key) != expected_small[wrong[0]][key]]
            print(f"random logs: {len(wrong)} of {len(small)} differ from the reference, first in {keys} for gap={gap} log={log}"[:700])
        else:
            score += 1
            print(f"random logs: all {len(small)} match the reference")

    if large is None:
        print(f"large log: not finished within {limit:.1f}s (the reference takes {min(timings):.2f}s)")
    elif large["large"] != expected_large:
        print("large log: the result differs from the reference")
    elif large["seconds"] > limit:
        print(f"large log: {large['seconds']:.1f}s, over the limit of {limit:.1f}s (the reference takes {min(timings):.2f}s)")
    else:
        score += 1
        print(f"large log: {large['seconds']:.2f}s, limit {limit:.1f}s (the reference takes {min(timings):.2f}s)")

    print(f"score: {score}/3")
    return 0 if score == 3 else 1


if __name__ == "__main__":
    if sys.argv[1:] == ["drive"]:
        drive()
    else:
        sys.exit(main())
