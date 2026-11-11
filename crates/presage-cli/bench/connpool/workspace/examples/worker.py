#!/usr/bin/env python3
"""How the report workers use the pool: many threads, few connections.

    python3 examples/worker.py
"""

import os
import sys
import threading
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from pool import ConnectionPool


class FakeConnection:
    """Stands in for a database connection."""

    opened = 0

    def __init__(self):
        time.sleep(0.02)  # the handshake
        FakeConnection.opened += 1
        self.queries = 0

    def execute(self, sql):
        self.queries += 1
        time.sleep(0.001)

    def close(self):
        pass


def worker(pool, reports):
    for _ in range(reports):
        with pool.connection(timeout=5.0) as conn:
            conn.execute("select * from report")


def main():
    pool = ConnectionPool(FakeConnection, max_size=4)
    threads = [threading.Thread(target=worker, args=(pool, 25)) for _ in range(12)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    print(f"opened {FakeConnection.opened} connections for a pool of {pool.max_size}: {pool.stats()}")
    pool.close()


if __name__ == "__main__":
    main()
