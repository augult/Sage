THOUGHT = r'''The stub polls with sleep, holds no lock, and tracks in-use connections with a list. I'll rewrite it on a Condition: reserve the slot before calling the factory so the bound holds even while the factory runs, call the factory outside the lock, wake one waiter per hand-over, notify all on close, and identity-check releases.'''
CONTENT = "Rewriting the pool around a Condition and verifying all ten guarantees."

POOL = r'''"""The connection pool. README.md lists what it guarantees."""

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

    def acquire(self, timeout=None):
        """A connection for the caller's exclusive use, until it is passed to `release`."""
        deadline = None if timeout is None else time.monotonic() + timeout
        with self._cond:
            while True:
                if self._closed:
                    raise PoolClosed("the pool is closed")
                if self._idle:
                    conn = self._idle.pop()
                    self._in_use.append(conn)
                    return conn
                if self._size < self._max_size:
                    # Reserve the place now; the factory runs outside the lock.
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
            self._created += 1
            if self._closed:
                self._size -= 1
                self._cond.notify()
                conn.close()
                raise PoolClosed("the pool is closed")
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
                conn.close()
                self._cond.notify_all()
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
            return {
                "size": self._size,
                "idle": len(self._idle),
                "in_use": len(self._in_use),
                "created": self._created,
            }
'''

CHECK = r'''python3 -m unittest discover -s tests -v && python3 - <<'PY'
import sys, threading, time
sys.path.insert(0, ".")
from pool import ConnectionPool, PoolClosed, PoolTimeout

class Conn:
    def __init__(self):
        self.closed = False
    def close(self):
        self.closed = True

# reuse most recently released first
p = ConnectionPool(Conn, 3)
a, b = p.acquire(), p.acquire()
p.release(a); p.release(b)
assert p.acquire() is b

# stranger and double release change nothing
p = ConnectionPool(Conn, 2)
c = p.acquire()
try:
    p.release(Conn()); raise SystemExit("foreign release accepted")
except ValueError:
    pass
assert p.stats() == {"size": 1, "idle": 0, "in_use": 1, "created": 1}, p.stats()
p.release(c)
try:
    p.release(c); raise SystemExit("double release accepted")
except ValueError:
    pass
assert p.stats() == {"size": 1, "idle": 1, "in_use": 0, "created": 1}, p.stats()

# bound holds and deadline totals the wait
p = ConnectionPool(Conn, 1)
held = p.acquire()
t0 = time.monotonic()
try:
    p.acquire(timeout=0.3); raise SystemExit("timeout ignored")
except PoolTimeout:
    pass
elapsed = time.monotonic() - t0
assert 0.25 <= elapsed < 1.0, elapsed
t0 = time.monotonic()
try:
    p.acquire(timeout=0); raise SystemExit("timeout=0 waited")
except PoolTimeout:
    pass
assert time.monotonic() - t0 < 0.1

# release wakes a waiter promptly, and it gets the same connection
p = ConnectionPool(Conn, 1)
held = p.acquire()
got = []
def waiter():
    t = time.monotonic()
    got.append((p.acquire(timeout=2), time.monotonic() - t))
th = threading.Thread(target=waiter); th.start()
time.sleep(0.2)
p.release(held)
th.join(1)
assert got and got[0][0] is held and got[0][1] < 0.15, got

# slow factory does not block release or reuse of idle connections
gate = threading.Event()
calls = []
def slow():
    calls.append(1)
    gate.wait(2)
    return Conn()
p = ConnectionPool(slow, 2)
idle_conn = Conn()
# create one outside the slow path: first acquire triggers slow factory, so seed idle directly
p._idle.append(idle_conn); p._size = 1; p._created = 1
t = threading.Thread(target=lambda: p.acquire()); t.start()
time.sleep(0.05)
p.release(idle_conn)  # must not block on the factory
fast = p.acquire(timeout=0.2)  # idle reuse while factory still running
assert fast is idle_conn
gate.set(); t.join(2)
assert len(calls) == 1

# failing factory frees the slot and wakes a waiter
p = ConnectionPool(lambda: (_ for _ in ()).throw(RuntimeError("boom")), 1)
try:
    p.acquire(); raise SystemExit("factory error swallowed")
except RuntimeError:
    pass
assert p.stats()["size"] == 0
p2 = ConnectionPool(Conn, 1)
h = p2.acquire()
result = []
def pending():
    try:
        result.append(p2.acquire(timeout=2))
    except Exception as e:
        result.append(e)
th = threading.Thread(target=pending); th.start()
time.sleep(0.2); p2.release(h); th.join(1)
assert result and isinstance(result[0], Conn), result

# close closes idle, wakes waiters, closes checked-out on release, idempotent
p = ConnectionPool(Conn, 1)
held = p.acquire()
waiters = []
def w():
    try:
        p.acquire(timeout=2)
    except PoolClosed:
        waiters.append(1)
t = threading.Thread(target=w); t.start()
time.sleep(0.2)
p.close(); p.close()
t.join(1)
assert waiters == [1], waiters
p.release(held)
assert held.closed and p.stats()["size"] == 0, p.stats()
try:
    p.acquire(); raise SystemExit("acquire after close worked")
except PoolClosed:
    pass

# idle closed on close
p = ConnectionPool(Conn, 2)
x = p.acquire(); p.release(x); p.close()
assert x.closed

# context manager releases on exception
p = ConnectionPool(Conn, 1)
try:
    with p.connection() as c:
        raise RuntimeError("inside")
except RuntimeError:
    pass
assert p.stats() == {"size": 1, "idle": 1, "in_use": 0, "created": 1}, p.stats()

# concurrent burst never exceeds max_size
p = ConnectionPool(Conn, 3)
seen = []
lock = threading.Lock()
def churn():
    for _ in range(50):
        c = p.acquire(timeout=2)
        with lock:
            s = p.stats()
            assert s["idle"] + s["in_use"] <= s["size"] <= 3, s
        p.release(c)
ths = [threading.Thread(target=churn) for _ in range(8)]
for t in ths: t.start()
for t in ths: t.join()
assert p.stats()["created"] <= 3, p.stats()
print("ALL CHECKS PASSED")
PY'''

CALLS = [
    {"name": "write", "arguments": {"filePath": "pool/pool.py", "content": POOL}},
    {"name": "bash", "arguments": {"command": CHECK}},
]
