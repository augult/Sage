THOUGHT = r'''Line 45 is `assert len(calls) == 1`: the worker thread simply reused the idle connection I seeded, so the factory was never entered and my slow-factory scenario proved nothing. I'll hold the idle connection in the main thread first, start a worker that must call the slow factory, then release and re-acquire while the factory is still blocked.'''
CONTENT = "That scenario never entered the factory. Fixing it so the worker really calls the slow factory."

CHECK = r'''python3 - <<'PY'
import sys, threading, time
sys.path.insert(0, ".")
from pool import ConnectionPool, PoolClosed, PoolTimeout

class Conn:
    def __init__(self):
        self.closed = False
    def close(self):
        self.closed = True

# release wakes a waiter promptly, and it gets the same connection
p = ConnectionPool(Conn, 1)
held = p.acquire()
got = []
def waiter():
    c = p.acquire(timeout=2)
    got.append((c, time.monotonic()))
th = threading.Thread(target=waiter); th.start()
time.sleep(0.2)
released_at = time.monotonic()
p.release(held)
th.join(1)
assert got and got[0][0] is held, got
wake = got[0][1] - released_at
assert wake < 0.1, f"wake took {wake:.3f}s"

# slow factory does not block release or reuse of idle connections
gate = threading.Event()
calls = []
def slow():
    calls.append(1)
    gate.wait(2)
    return Conn()
p = ConnectionPool(slow, 2)
seed = Conn()
p._idle.append(seed); p._size = 1; p._created = 1
held = p.acquire()  # main holds the only idle connection
worker = []
def factory_thread():
    worker.append(p.acquire())
t = threading.Thread(target=factory_thread); t.start()
deadline = time.monotonic() + 1
while not calls and time.monotonic() < deadline:
    time.sleep(0.01)
assert len(calls) == 1, "worker never entered the factory"
t0 = time.monotonic()
p.release(held)  # must not block on the factory
fast = p.acquire(timeout=0.2)  # idle reuse while factory still blocked
assert fast is seed
assert time.monotonic() - t0 < 0.15, "release/acquire blocked on the factory"
gate.set(); t.join(2)
assert len(worker) == 1 and not t.is_alive()
assert p.stats()["size"] == 2, p.stats()

# failing factory frees the slot and wakes a waiter
p = ConnectionPool(lambda: (_ for _ in ()).throw(RuntimeError("boom")), 1)
try:
    p.acquire(); raise SystemExit("factory error swallowed")
except RuntimeError:
    pass
assert p.stats()["size"] == 0 and p.stats()["created"] == 0
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

# timeout=0 with a free slot still succeeds
p = ConnectionPool(Conn, 1)
c0 = p.acquire(timeout=0)
assert c0 is not None

# concurrent burst never exceeds max_size and stays a consistent snapshot
p = ConnectionPool(Conn, 3)
lock = threading.Lock()
def churn():
    for _ in range(50):
        c = p.acquire(timeout=2)
        with lock:
            s = p.stats()
            assert s["idle"] + s["in_use"] <= s["size"] <= 3, s
        time.sleep(0.001)
        p.release(c)
ths = [threading.Thread(target=churn) for _ in range(8)]
for t in ths: t.start()
for t in ths: t.join()
s = p.stats()
assert s["created"] <= 3 and s["in_use"] == 0, s
print("ALL CHECKS PASSED", s)
PY'''

CALLS = [
    {"name": "bash", "arguments": {"command": CHECK}},
]
