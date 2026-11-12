import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from fetchkit import RetryError, backoff_delays, call_with_retry


class Flaky:
    """Fails `failures` times, then returns "done"."""

    def __init__(self, failures):
        self.failures = failures
        self.calls = 0

    def __call__(self):
        self.calls += 1
        if self.calls <= self.failures:
            raise OSError(f"failure {self.calls}")
        return "done"


class RetryTest(unittest.TestCase):
    def test_succeeds_on_the_last_attempt(self):
        fn = Flaky(2)
        self.assertEqual(call_with_retry(fn, attempts=3, sleep=lambda _: None), "done")
        self.assertEqual(fn.calls, 3)

    def test_gives_up_after_the_last_attempt(self):
        fn = Flaky(5)
        with self.assertRaises(RetryError):
            call_with_retry(fn, attempts=3, sleep=lambda _: None)
        self.assertEqual(fn.calls, 3)

    def test_default_is_three_attempts(self):
        fn = Flaky(9)
        with self.assertRaises(RetryError):
            call_with_retry(fn, sleep=lambda _: None)
        self.assertEqual(fn.calls, 3)

    def test_delays(self):
        self.assertEqual(backoff_delays(4, base=1.0), [1.0, 2.0, 4.0])


if __name__ == "__main__":
    unittest.main()
