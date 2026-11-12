"""The connection pool. README.md lists what it guarantees."""

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
        self._in_use = {}
        # Connections that exist or are being created: a place is taken before `factory` runs.
        self._size = 0
        self._creating = 0
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
                    self._in_use[id(conn)] = conn
                    return conn
                if self._size < self._max_size:
                    self._size += 1
                    self._creating += 1
                    break
                remaining = None if deadline is None else deadline - time.monotonic()
                if remaining is not None and remaining <= 0:
                    raise PoolTimeout(f"no connection within {timeout}s")
                self._cond.wait(remaining)
        try:
            conn = self._factory()
        except BaseException:
            with self._cond:
                self._size -= 1
                self._creating -= 1
                self._cond.notify()
            raise
        with self._cond:
            self._creating -= 1
            self._created += 1
            if self._closed:
                self._size -= 1
                closed = True
            else:
                self._in_use[id(conn)] = conn
                closed = False
        if closed:
            conn.close()
            raise PoolClosed("the pool is closed")
        return conn

    def release(self, conn):
        """Gives a connection back."""
        with self._cond:
            if self._in_use.get(id(conn)) is not conn:
                raise ValueError("this connection is not checked out of the pool")
            del self._in_use[id(conn)]
            if self._closed:
                self._size -= 1
            else:
                self._idle.append(conn)
                self._cond.notify()
                return
        conn.close()

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
                "size": self._size - self._creating,
                "idle": len(self._idle),
                "in_use": len(self._in_use),
                "created": self._created,
            }
