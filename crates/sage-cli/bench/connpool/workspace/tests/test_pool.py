import os
import sys
import threading
import time
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from pool import ConnectionPool, PoolClosed, PoolTimeout


class Conn:
    def __init__(self):
        self.closed = False

    def close(self):
        self.closed = True


class SingleThreadTest(unittest.TestCase):
    def test_reuses_the_most_recently_released(self):
        pool = ConnectionPool(Conn, max_size=3)
        first, second = pool.acquire(), pool.acquire()
        pool.release(first)
        pool.release(second)
        self.assertIs(pool.acquire(), second)
        self.assertEqual(pool.stats(), {"size": 2, "idle": 1, "in_use": 1, "created": 2})

    def test_times_out_when_exhausted(self):
        pool = ConnectionPool(Conn, max_size=1)
        pool.acquire()
        with self.assertRaises(PoolTimeout):
            pool.acquire(timeout=0)

    def test_close(self):
        pool = ConnectionPool(Conn, max_size=2)
        conn = pool.acquire()
        pool.release(conn)
        pool.close()
        self.assertTrue(conn.closed)
        with self.assertRaises(PoolClosed):
            pool.acquire()


class ManyThreadsTest(unittest.TestCase):
    def test_never_more_than_max_size(self):
        opened = []

        def slow_factory():
            time.sleep(0.01)
            conn = Conn()
            opened.append(conn)
            return conn

        pool = ConnectionPool(slow_factory, max_size=2)

        def work():
            for _ in range(5):
                with pool.connection(timeout=10.0):
                    time.sleep(0.001)

        threads = [threading.Thread(target=work) for _ in range(8)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
        self.assertLessEqual(len(opened), 2)
        self.assertEqual(pool.stats()["in_use"], 0)


if __name__ == "__main__":
    unittest.main()
