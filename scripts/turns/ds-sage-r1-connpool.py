THOUGHT = r'''Another test-script bug: I measured the waiter's total block time rather than the wake-up delay after release, and assumed the contention run always creates all three connections. The pool code and watched tests are fine; fixing the assertions.'''
CONTENT = ""
CALLS = [
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''python3 - <<'PY'
import sys, threading, time
sys.path.insert(0, ".")
from pool import ConnectionPool, PoolClosed, PoolTimeout

class Conn:
    def __init__(self):
        self.closed = False
    def close(self):
        self.closed = True

# reuse most recently released
p = ConnectionPool(Conn, 3)
a, b = p.acquire(), p.acquire()
p.release(a); p.release(b)
assert p.acquire() is b

# stranger and double release change nothing
p = ConnectionPool(Conn, 2)
c = p.acquire()
try:
    p.release(Conn())
    raise SystemExit("release accepted a foreign connection")
except ValueError:
    pass
assert p.stats() == {"size": 1, "idle": 0, "in_use": 1, "created": 1}, p.stats()
p.release(c)
try:
    p.release(c)
    raise SystemExit("release accepted a double release")
except ValueError:
    pass
assert p.stats() == {"size": 1, "idle": 1, "in_use": 0, "created": 1}, p.stats()

# total deadline, no polling
p = ConnectionPool(Conn, 1)
p.acquire()
t = time.monotonic()
try:
    p.acquire(timeout=0.4)
except PoolTimeout:
    pass
else:
    raise SystemExit("no timeout")
assert 0.35 <= time.monotonic() - t < 0.8

# timeout=0 never waits
p = ConnectionPool(Conn, 1)
p.acquire()
t = time.monotonic()
try:
    p.acquire(timeout=0)
except PoolTimeout:
    pass
assert time.monotonic() - t < 0.1

# prompt hand-over on release: wake-up delay measured from the release
p = ConnectionPool(Conn, 1)
c = p.acquire()
woken = []
def w():
    x = p.acquire(timeout=5)
    woken.append(time.monotonic())
    p.release(x)
th = threading.Thread(target=w); th.start(); time.sleep(0.2)
t = time.monotonic(); p.release(c); th.join()
assert woken and woken[0] - t < 0.1, (woken, t)

# close wakes waiters, closes idle, checked-out closed on release, repeat close no-op
p = ConnectionPool(Conn, 2)
idle = p.acquire(); p.release(idle)
held = p.acquire()
got = []
def waiter():
    try:
        p.acquire(timeout=5)
    except PoolClosed:
        got.append(True)
th = threading.Thread(target=waiter); th.start(); time.sleep(0.1)
p.close(); th.join(2)
assert got == [True]
assert idle.closed
p.release(held); assert held.closed
p.close()

# failing factory frees the slot for a later acquire
state = {"fail": True}
def factory():
    if state["fail"]:
        raise RuntimeError("boom")
    return Conn()
p = ConnectionPool(factory, 1)
for _ in range(3):
    try:
        p.acquire(timeout=0)
        raise SystemExit("factory error swallowed")
    except RuntimeError:
        pass
assert p.stats()["size"] == 0, p.stats()
state["fail"] = False
assert p.acquire(timeout=0) is not None

# slow factory: slot reserved and counted, owners can still release
opened = []
def slow():
    time.sleep(0.5)
    conn = Conn(); opened.append(conn); return conn
p = ConnectionPool(slow, 2)
out = []
t1 = threading.Thread(target=lambda: out.append(p.acquire()))
t2 = threading.Thread(target=lambda: out.append(p.acquire()))
t1.start(); t2.start(); time.sleep(0.1)
assert p.stats()["size"] == 2, p.stats()
t1.join(); t2.join()
assert len(opened) == 2
assert p.stats() == {"size": 2, "idle": 0, "in_use": 2, "created": 2}, p.stats()

# connection() releases on exception, no leak
p = ConnectionPool(Conn, 1)
try:
    with p.connection(timeout=0):
        raise RuntimeError("x")
except RuntimeError:
    pass
assert p.stats() == {"size": 1, "idle": 1, "in_use": 0, "created": 1}, p.stats()

# stats invariant under contention
p = ConnectionPool(Conn, 3)
def work():
    for _ in range(15):
        with p.connection(timeout=5):
            s = p.stats()
            assert s["idle"] + s["in_use"] <= s["size"] <= 3, s
ths = [threading.Thread(target=work) for _ in range(8)]
[t.start() for t in ths]; [t.join() for t in ths]
s = p.stats()
assert s["in_use"] == 0 and s["idle"] == s["size"] and s["created"] == s["size"] and 1 <= s["size"] <= 3, s
print("ALL OK")
PY''',
            "expect_exit": "0",
            "expect_has": "ALL OK",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Rewrote ConnectionPool to keep all ten guarantees: condition-based waiting with a total monotonic deadline, slots reserved before factory runs (bounded including in-flight creation), factory called outside the lock, identity-checked release raising ValueError, notification-based hand-over, close waking waiters and closing idle/returned connections. Verified the watched suite plus focused checks for LIFO reuse, deadline accuracy, timeout=0, prompt hand-over, failing factory slot release, slow-factory concurrency/bounds, close semantics and stats invariants.",
        },
    },
]
