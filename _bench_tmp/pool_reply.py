import json
import sys

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
        self._condition = threading.Condition()
        self._idle = []
        self._in_use = {}
        self._size = 0
        self._created = 0
        self._closed = False

    @property
    def max_size(self):
        return self._max_size

    def acquire(self, timeout=None):
        """A connection for the caller alone, until it is passed to `release`."""
        deadline = None if timeout is None else time.monotonic() + timeout
        with self._condition:
            while True:
                if self._closed:
                    raise PoolClosed("the pool is closed")
                if self._idle:
                    conn = self._idle.pop()
                    self._in_use[id(conn)] = conn
                    return conn
                if self._size < self._max_size:
                    # reserve the slot; the factory call happens outside the lock
                    self._size += 1
                    break
                if deadline is None:
                    self._condition.wait()
                    continue
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise PoolTimeout(f"no connection within {timeout}s")
                self._condition.wait(remaining)

        try:
            conn = self._factory()
        except BaseException:
            with self._condition:
                self._size -= 1
                self._condition.notify_all()
            raise

        with self._condition:
            self._created += 1
            if not self._closed:
                self._in_use[id(conn)] = conn
                return conn
            self._size -= 1

        conn.close()
        raise PoolClosed("the pool is closed")

    def release(self, conn):
        """Gives a connection back."""
        with self._condition:
            if self._in_use.get(id(conn)) is not conn:
                raise ValueError("connection is not checked out from this pool")
            del self._in_use[id(conn)]
            closed = self._closed
            if closed:
                self._size -= 1
            else:
                self._idle.append(conn)
            self._condition.notify_all()
        if closed:
            conn.close()

    @contextlib.contextmanager
    def connection(self, timeout=None):
        conn = self.acquire(timeout)
        try:
            yield conn
        finally:
            self.release(conn)

    def close(self):
        with self._condition:
            if self._closed:
                return
            self._closed = True
            idle, self._idle = self._idle, []
            self._size -= len(idle)
            self._condition.notify_all()
        for conn in idle:
            conn.close()

    def stats(self):
        with self._condition:
            return {
                "size": self._size,
                "idle": len(self._idle),
                "in_use": len(self._in_use),
                "created": self._created,
            }
'''

WORK = "/tmp/sage-hopliteloop.fazhjc/ds-oc-connpool-1/home/bench/20261122-233548-83ba/work/0-connpool-ext-opencode-0"

payload = {
    "calls": [
        {"name": "write", "arguments": {"filePath": WORK + "/pool/pool.py", "content": POOL}}
    ]
}
json.dump(payload, sys.stdout)
