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
        self.holder = None

    def close(self):
        self.closed = True


class Task(threading.Thread):
    """Runs `fn` in a daemon thread and keeps what it returned or raised."""

    def __init__(self, fn):
        super().__init__(daemon=True)
        self.fn = fn
        self.result = None
        self.error = None
        self.start()

    def run(self):
        try:
            self.result = self.fn()
        except BaseException as error:  # noqa: BLE001
            self.error = error

    def finished(self, within=5.0):
        self.join(within)
        return not self.is_alive()


class GuaranteeTest(unittest.TestCase):
    def test_1_bounded(self):
        opened = []

        def slow_factory():
            time.sleep(0.005)
            conn = Conn()
            opened.append(conn)
            return conn

        pool = ConnectionPool(slow_factory, max_size=3)

        def work():
            for _ in range(10):
                with pool.connection(timeout=30.0):
                    time.sleep(0.0005)

        tasks = [Task(work) for _ in range(12)]
        for task in tasks:
            self.assertTrue(task.finished(60.0))
            self.assertIsNone(task.error)
        self.assertLessEqual(len(opened), 3)
        stats = pool.stats()
        self.assertEqual((stats["in_use"], stats["idle"], stats["created"]), (0, stats["size"], len(opened)))

    def test_2_exclusive_under_stress(self):
        pool = ConnectionPool(Conn, max_size=4)
        clashes = []
        bad_stats = []
        done = threading.Event()

        def work(me):
            for round_ in range(300):
                conn = pool.acquire(timeout=60.0)
                if conn.holder is not None:
                    clashes.append((me, conn.holder))
                conn.holder = me
                if round_ % 7 == 0:
                    time.sleep(0)
                conn.holder = None
                pool.release(conn)

        def watch():
            while not done.is_set():
                stats = pool.stats()
                if not stats["idle"] + stats["in_use"] <= stats["size"] <= 4:
                    bad_stats.append(stats)

        old = sys.getswitchinterval()
        sys.setswitchinterval(1e-5)
        try:
            watcher = Task(watch)
            tasks = [Task(lambda me=me: work(me)) for me in range(16)]
            for task in tasks:
                self.assertTrue(task.finished(90.0))
                self.assertIsNone(task.error)
            done.set()
            self.assertTrue(watcher.finished())
        finally:
            done.set()
            sys.setswitchinterval(old)
        self.assertEqual(clashes, [])
        self.assertEqual(bad_stats[:3], [])
        stats = pool.stats()
        self.assertLessEqual(stats["created"], 4)
        self.assertEqual((stats["in_use"], stats["idle"]), (0, stats["size"]))

    def test_3_reuse_first(self):
        pool = ConnectionPool(Conn, max_size=3)
        a, b, c = pool.acquire(), pool.acquire(), pool.acquire()
        pool.release(b)
        pool.release(a)
        pool.release(c)
        self.assertIs(pool.acquire(), c)
        self.assertIs(pool.acquire(), a)
        self.assertEqual(pool.stats()["created"], 3)

    def test_4_deadline(self):
        pool = ConnectionPool(Conn, max_size=1)
        pool.acquire()
        started = time.monotonic()
        with self.assertRaises(PoolTimeout):
            pool.acquire(timeout=0.3)
        waited = time.monotonic() - started
        self.assertGreaterEqual(waited, 0.28)
        self.assertLess(waited, 3.0)
        started = time.monotonic()
        with self.assertRaises(PoolTimeout):
            pool.acquire(timeout=0)
        self.assertLess(time.monotonic() - started, 0.2)

    def test_5_prompt_hand_over(self):
        pool = ConnectionPool(Conn, max_size=1)
        conn = pool.acquire()
        started = time.monotonic()
        for _ in range(20):
            waiting = threading.Event()

            def take():
                waiting.set()
                got = pool.acquire(timeout=30.0)
                pool.release(got)
                return got

            task = Task(take)
            waiting.wait(5.0)
            time.sleep(0.01)
            pool.release(conn)
            self.assertTrue(task.finished(30.0))
            self.assertIs(task.result, conn)
            conn = pool.acquire(timeout=5.0)
        self.assertLess(time.monotonic() - started, 2.5, "waiters must be woken by release, not by a timer")

    def test_6_slow_factory_blocks_nobody(self):
        entered, gate = threading.Event(), threading.Event()
        calls = []

        def factory():
            calls.append(1)
            if len(calls) == 2:
                entered.set()
                gate.wait(30.0)
            return Conn()

        pool = ConnectionPool(factory, max_size=2)
        first = pool.acquire()
        slow = Task(lambda: pool.acquire(timeout=30.0))
        self.assertTrue(entered.wait(5.0))
        try:
            release = Task(lambda: pool.release(first))
            self.assertTrue(release.finished(3.0), "release waited for the factory")
            self.assertIsNone(release.error)
            again = Task(lambda: pool.acquire(timeout=3.0))
            self.assertTrue(again.finished(5.0), "acquire of an idle connection waited for the factory")
            self.assertIs(again.result, first)
            self.assertFalse(slow.finished(0.05))
        finally:
            gate.set()
        self.assertTrue(slow.finished())
        self.assertIsNot(slow.result, first)
        self.assertIsInstance(slow.result, Conn)
        self.assertEqual(pool.stats(), {"size": 2, "idle": 0, "in_use": 2, "created": 2})

    def test_7_failing_factory_costs_nothing(self):
        calls = []

        def factory():
            calls.append(1)
            if len(calls) == 1:
                raise RuntimeError("connection refused")
            return Conn()

        pool = ConnectionPool(factory, max_size=1)
        with self.assertRaises(RuntimeError):
            pool.acquire()
        self.assertEqual(pool.stats(), {"size": 0, "idle": 0, "in_use": 0, "created": 0})
        task = Task(lambda: pool.acquire(timeout=2.0))
        self.assertTrue(task.finished())
        self.assertIsInstance(task.result, Conn)
        self.assertEqual(pool.stats(), {"size": 1, "idle": 0, "in_use": 1, "created": 1})

    def test_7_a_waiter_gets_the_freed_place(self):
        entered, gate = threading.Event(), threading.Event()
        calls = []

        def factory():
            calls.append(1)
            if len(calls) == 1:
                entered.set()
                gate.wait(30.0)
                raise RuntimeError("connection refused")
            return Conn()

        pool = ConnectionPool(factory, max_size=1)
        failing = Task(pool.acquire)
        self.assertTrue(entered.wait(5.0))
        waiter = Task(lambda: pool.acquire(timeout=40.0))
        time.sleep(0.1)
        gate.set()
        self.assertTrue(failing.finished())
        self.assertIsInstance(failing.error, RuntimeError)
        self.assertTrue(waiter.finished(8.0), "the waiting acquire was not told about the freed place")
        self.assertIsInstance(waiter.result, Conn)

    def test_8_only_what_was_handed_out_comes_back(self):
        pool = ConnectionPool(Conn, max_size=2)
        conn = pool.acquire()
        pool.release(conn)
        before = pool.stats()
        with self.assertRaises(ValueError):
            pool.release(conn)
        with self.assertRaises(ValueError):
            pool.release(Conn())
        self.assertEqual(pool.stats(), before)
        self.assertIs(pool.acquire(), conn)
        other = pool.acquire(timeout=1.0)
        self.assertIsNot(other, conn)
        with self.assertRaises(PoolTimeout):
            pool.acquire(timeout=0)

    def test_9_close(self):
        pool = ConnectionPool(Conn, max_size=3)
        idle, busy = pool.acquire(), pool.acquire()
        pool.release(idle)
        pool.close()
        self.assertTrue(idle.closed)
        self.assertFalse(busy.closed)
        with self.assertRaises(PoolClosed):
            pool.acquire(timeout=0)
        pool.release(busy)
        self.assertTrue(busy.closed)
        stats = pool.stats()
        self.assertEqual((stats["size"], stats["idle"], stats["in_use"]), (0, 0, 0))
        pool.close()

    def test_9_close_wakes_the_waiters(self):
        pool = ConnectionPool(Conn, max_size=1)
        pool.acquire()
        waiters = [Task(pool.acquire), Task(lambda: pool.acquire(timeout=60.0))]
        time.sleep(0.2)
        pool.close()
        for waiter in waiters:
            self.assertTrue(waiter.finished(8.0), "a waiting acquire was not woken by close")
            self.assertIsInstance(waiter.error, PoolClosed)

    def test_the_with_block_releases_on_error(self):
        pool = ConnectionPool(Conn, max_size=1)
        with self.assertRaises(KeyError):
            with pool.connection() as conn:
                raise KeyError("boom")
        self.assertEqual(pool.stats(), {"size": 1, "idle": 1, "in_use": 0, "created": 1})
        self.assertIs(pool.acquire(timeout=0), conn)


if __name__ == "__main__":
    unittest.main()
