import json
import sys

WORK = "/tmp/sage-hopliteloop.fazhjc/ds-oc-connpool-1/home/bench/20261122-233548-83ba/work/0-connpool-ext-opencode-0"

CHECK = r'''import sys
import threading
import time

sys.path.insert(0, "WORKDIR")

from pool import ConnectionPool, PoolClosed, PoolTimeout


class Conn:
    def __init__(self):
        self.closed = False

    def close(self):
        self.closed = True


failures = []


def check(name, fn):
    try:
        fn()
    except Exception as exc:
        failures.append(name)
        print("FAIL", name, repr(exc))
    else:
        print("ok  ", name)


def g1_bounded():
    opened = []
    lock = threading.Lock()

    def factory():
        time.sleep(0.005)
        conn = Conn()
        with lock:
            opened.append(conn)
        return conn

    pool = ConnectionPool(factory, max_size=3)

    def work():
        for _ in range(10):
            with pool.connection(timeout=10):
                time.sleep(0.001)

    threads = [threading.Thread(target=work) for _ in range(12)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert len(opened) <= 3, len(opened)
    assert pool.stats()["size"] <= 3


def g2_exclusive():
    holders = []
    lock = threading.Lock()
    violations = []

    pool = ConnectionPool(Conn, max_size=2)

    def work():
        for _ in range(50):
            conn = pool.acquire(timeout=10)
            with lock:
                holders.append(conn)
                if len(holders) > 2:
                    violations.append("more than max_size holders")
                if holders.count(conn) > 1:
                    violations.append("same conn twice")
            time.sleep(0.0005)
            with lock:
                holders.remove(conn)
            pool.release(conn)

    threads = [threading.Thread(target=work) for _ in range(8)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert not violations, violations


def g3_lifo():
    pool = ConnectionPool(Conn, max_size=3)
    a, b, c = pool.acquire(), pool.acquire(), pool.acquire()
    pool.release(a)
    pool.release(c)
    pool.release(b)
    assert pool.acquire() is b
    assert pool.acquire() is c
    assert pool.acquire() is a


def g4_deadline():
    pool = ConnectionPool(Conn, max_size=1)
    held = pool.acquire()
    start = time.monotonic()
    try:
        pool.acquire(timeout=0.2)
    except PoolTimeout:
        pass
    else:
        raise AssertionError("no PoolTimeout")
    elapsed = time.monotonic() - start
    assert 0.19 <= elapsed <= 0.35, elapsed
    pool.release(held)
    start = time.monotonic()
    pool.acquire(timeout=0)
    assert time.monotonic() - start < 0.05


def g5_handover():
    pool = ConnectionPool(Conn, max_size=1)
    held = pool.acquire()
    got = []

    def waiter():
        got.append(pool.acquire(timeout=5))

    t = threading.Thread(target=waiter)
    t.start()
    time.sleep(0.15)
    start = time.monotonic()
    pool.release(held)
    t.join(2)
    latency = time.monotonic() - start
    assert got and got[0] is held
    assert latency < 0.05, latency


def g6_slow_factory():
    def factory():
        time.sleep(0.5)
        return Conn()

    pool = ConnectionPool(factory, max_size=2)
    held = pool.acquire()  # instant: first slot, slow factory
    created = []
    t = threading.Thread(target=lambda: created.append(pool.acquire(timeout=5)))
    t.start()
    time.sleep(0.05)
    start = time.monotonic()
    pool.release(held)
    again = pool.acquire(timeout=1)
    took = time.monotonic() - start
    assert again is held
    assert took < 0.2, took
    t.join(3)
    assert created


def g7_failing_factory():
    calls = []

    def factory():
        calls.append(1)
        if len(calls) == 1:
            raise RuntimeError("boom")
        return Conn()

    pool = ConnectionPool(factory, max_size=1)
    try:
        pool.acquire(timeout=1)
    except RuntimeError:
        pass
    else:
        raise AssertionError("exception not propagated")
    assert pool.stats()["size"] == 0, pool.stats()
    conn = pool.acquire(timeout=0)
    assert conn is not None
    pool.release(conn)

    # a failing factory frees the slot for a waiting acquire
    gate = threading.Event()

    def slow_fail():
        gate.wait(2)
        raise RuntimeError("late")

    pool2 = ConnectionPool(slow_fail, max_size=1)
    errors = []

    def first():
        try:
            pool2.acquire(timeout=2)
        except RuntimeError as exc:
            errors.append(exc)

    t1 = threading.Thread(target=first)
    t1.start()
    time.sleep(0.05)
    gate.set()
    deadline = time.monotonic() + 1.5
    while pool2.stats()["size"] != 0 and time.monotonic() < deadline:
        time.sleep(0.01)
    t1.join(3)
    assert errors
    assert pool2.stats()["size"] == 0, pool2.stats()


def g8_strangers():
    pool = ConnectionPool(Conn, max_size=2)
    conn = pool.acquire()
    before = pool.stats()
    try:
        pool.release(Conn())
    except ValueError:
        pass
    else:
        raise AssertionError("stranger accepted")
    assert pool.stats() == before
    pool.release(conn)
    try:
        pool.release(conn)
    except ValueError:
        pass
    else:
        raise AssertionError("double release accepted")
    assert pool.stats()["in_use"] == 0


def g9_close():
    pool = ConnectionPool(Conn, max_size=1)
    held = pool.acquire()
    woke = []

    def waiter():
        try:
            pool.acquire(timeout=5)
        except PoolClosed:
            woke.append("closed")

    t = threading.Thread(target=waiter)
    t.start()
    time.sleep(0.1)
    pool.close()
    t.join(2)
    assert woke == ["closed"], woke
    assert not held.closed
    try:
        pool.acquire()
    except PoolClosed:
        pass
    else:
        raise AssertionError("acquire after close")
    pool.release(held)
    assert held.closed
    stats = pool.stats()
    assert stats["size"] == 0 and stats["idle"] == 0 and stats["in_use"] == 0, stats
    pool.close()

    other = ConnectionPool(Conn, max_size=2)
    a = other.acquire()
    b = other.acquire()
    other.release(a)
    other.close()
    assert a.closed and not b.closed
    other.release(b)
    assert b.closed
    assert other.stats()["size"] == 0, other.stats()


def g11_close_during_create():
    gate = threading.Event()
    created = []

    def factory():
        gate.wait(2)
        conn = Conn()
        created.append(conn)
        return conn

    pool = ConnectionPool(factory, max_size=1)
    errors = []

    def acquirer():
        try:
            pool.acquire(timeout=5)
        except PoolClosed:
            errors.append("closed")

    t = threading.Thread(target=acquirer)
    t.start()
    time.sleep(0.05)
    pool.close()
    gate.set()
    t.join(3)
    assert errors == ["closed"], errors
    assert pool.stats()["size"] == 0, pool.stats()
    assert all(c.closed for c in created)


def g12_context_release():
    pool = ConnectionPool(Conn, max_size=1)
    try:
        with pool.connection(timeout=1) as conn:
            raise RuntimeError("body")
    except RuntimeError:
        pass
    else:
        raise AssertionError("body exception swallowed")
    assert pool.stats()["in_use"] == 0
    assert pool.stats()["idle"] == 1
    again = pool.acquire(timeout=0)
    assert again is conn


def g10_stats():
    pool = ConnectionPool(Conn, max_size=3)
    a = pool.acquire()
    b = pool.acquire()
    pool.release(a)
    stats = pool.stats()
    assert set(stats) == {"size", "idle", "in_use", "created"}
    assert stats == {"size": 2, "idle": 1, "in_use": 1, "created": 2}, stats
    pool.release(b)
    bad = []

    def sample():
        for _ in range(200):
            s = pool.stats()
            if not (s["idle"] + s["in_use"] <= s["size"] <= 3):
                bad.append(s)

    def churn():
        for _ in range(50):
            with pool.connection(timeout=5):
                pass

    threads = [threading.Thread(target=sample) for _ in range(3)]
    threads += [threading.Thread(target=churn) for _ in range(4)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert not bad, bad[:3]
    assert pool.stats()["created"] >= 2


for name, fn in [
    ("g1 bounded", g1_bounded),
    ("g2 exclusive", g2_exclusive),
    ("g3 reuse first", g3_lifo),
    ("g4 deadline", g4_deadline),
    ("g5 handover", g5_handover),
    ("g6 slow factory", g6_slow_factory),
    ("g7 failing factory", g7_failing_factory),
    ("g8 strangers", g8_strangers),
    ("g9 close", g9_close),
    ("g10 stats", g10_stats),
    ("g11 close during create", g11_close_during_create),
    ("g12 context release", g12_context_release),
]:
    check(name, fn)

print("FAILURES", failures)
'''

CHECK = CHECK.replace("WORKDIR", WORK)

payload = {
    "calls": [
        {
            "name": "write",
            "arguments": {"filePath": "/tmp/opencode/check_pool.py", "content": CHECK},
        },
        {
            "name": "bash",
            "arguments": {
                "command": "python3 -m unittest discover -s tests -v 2>&1 | tail -40",
                "workdir": WORK,
            },
        },
        {
            "name": "bash",
            "arguments": {
                "command": "python3 /tmp/opencode/check_pool.py 2>&1",
                "workdir": WORK,
                "timeout": 120000,
            },
        },
        {
            "name": "bash",
            "arguments": {"command": "python3 examples/worker.py 2>&1", "workdir": WORK},
        },
    ]
}
json.dump(payload, sys.stdout)
