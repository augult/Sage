"""The connection pool. README.md lists what it guarantees."""

import contextlib
import time

from .errors import PoolClosed, PoolTimeout

POLL_SECONDS = 0.25


class ConnectionPool:
    def __init__(self, factory, max_size=4):
        if max_size < 1:
            raise ValueError("max_size must be at least 1")
        self._factory = factory
        self._max_size = max_size
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
        waited = 0.0
        while True:
            if self._closed:
                raise PoolClosed("the pool is closed")
            if self._idle:
                conn = self._idle.pop()
                self._in_use.append(conn)
                return conn
            if self._size < self._max_size:
                conn = self._factory()
                self._size += 1
                self._created += 1
                self._in_use.append(conn)
                return conn
            if timeout is not None and waited >= timeout:
                raise PoolTimeout(f"no connection within {timeout}s")
            time.sleep(POLL_SECONDS)
            waited += POLL_SECONDS

    def release(self, conn):
        """Gives a connection back."""
        if conn in self._in_use:
            self._in_use.remove(conn)
        if self._closed:
            conn.close()
            self._size -= 1
        else:
            self._idle.append(conn)

    @contextlib.contextmanager
    def connection(self, timeout=None):
        conn = self.acquire(timeout)
        yield conn
        self.release(conn)

    def close(self):
        self._closed = True
        for conn in self._idle:
            conn.close()
        self._size -= len(self._idle)
        self._idle = []

    def stats(self):
        return {"size": self._size, "idle": len(self._idle), "in_use": len(self._in_use), "created": self._created}
