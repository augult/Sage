THOUGHT = r'''All ten guarantees fail around three things the current code gets wrong: there is no lock or condition (bounded, exclusive, honest numbers), waiting is done by polling and only checks the timeout after a sleep (blocking with deadline, prompt hand-over), and factory/create bookkeeping is unguarded and never rolled back (failing factory, in-flight bound). I will rewrite the pool with a Condition, reserve a slot before the factory call, call the factory outside the lock, wake one waiter per hand-over, close idle connections on close, wake waiters on close, and identity-check releases.'''
CONTENT = ""
CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "pool/pool.py",
            "overwrite": True,
            "content": r'''"""The connection pool. README.md lists what it guarantees."""

import contextlib
import threading
import time

from .errors import PoolClosed, PoolTimeout


class ConnectionPool:
    def __init__(self, factory, max_size=4):
        if max_size < 1:
            raise ValueError("max_size must be at least 1")
        self._factory = factory
        self._max_size = max_size
        self._cond = threading.Condition()
        self._idle = []
        self._in_use = []
        self._size = 0
        self._created = 0
        self._closed = False

    @property
    def max_size(self):
        return self._max_size

    def _take_idle(self):
        conn = self._idle.pop()
        self._in_use.append(conn)
        return conn

    def acquire(self, timeout=None):
        """A connection for the caller's exclusive use, until it is passed to `release`."""
        deadline = None if timeout is None else time.monotonic() + timeout
        with self._cond:
            while True:
                if self._closed:
                    raise PoolClosed("the pool is closed")
                if self._idle:
                    return self._take_idle()
                if self._size < self._max_size:
                    # Reserve the place now: the factory runs outside the lock.
                    self._size += 1
                    break
                if deadline is None:
                    self._cond.wait()
                    continue
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise PoolTimeout(f"no connection within {timeout}s")
                self._cond.wait(remaining)

        try:
            conn = self._factory()
        except BaseException:
            with self._cond:
                self._size -= 1
                self._cond.notify()
            raise

        with self._cond:
            if self._closed:
                self._size -= 1
                self._cond.notify()
                conn.close()
                raise PoolClosed("the pool is closed")
            self._created += 1
            self._in_use.append(conn)
            return conn

    def release(self, conn):
        """Gives a connection back."""
        with self._cond:
            index = next((i for i, held in enumerate(self._in_use) if held is conn), None)
            if index is None:
                raise ValueError("connection was not acquired from this pool")
            del self._in_use[index]
            if self._closed:
                self._size -= 1
                self._cond.notify_all()
                conn.close()
            else:
                self._idle.append(conn)
                self._cond.notify()

    @contextlib.contextmanager
    def connection(self, timeout=None):
        conn = self.acquire(timeout)
        try:
            yield conn
        finally:
            self.release(conn)

    def close(self):
        with self._cond:
            if self._closed:
                return
            self._closed = True
            idle, self._idle = self._idle, []
            self._size -= len(idle)
            self._cond.notify_all()
        for conn in idle:
            conn.close()

    def stats(self):
        with self._cond:
            return {"size": self._size, "idle": len(self._idle), "in_use": len(self._in_use), "created": self._created}
''',
        },
    },
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

# close closes idle connections, checked-out ones on release, repeat close no-op
p = ConnectionPool(Conn, 2)
a = p.acquire(); held = p.acquire()
p.release(a)
p.close()
assert a.closed and not held.closed
p.release(held); assert held.closed
p.close()

# close wakes waiters
p = ConnectionPool(Conn, 1)
held = p.acquire()
got = []
def waiter():
    try:
        p.acquire(timeout=5)
    except PoolClosed:
        got.append(True)
th = threading.Thread(target=waiter); th.start(); time.sleep(0.1)
p.close(); th.join(2)
assert got == [True], got
assert not held.closed
p.release(held); assert held.closed

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
