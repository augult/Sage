"""Calling something again when it fails."""

import time


class RetryError(Exception):
    """Every try failed. `attempts` is how many tries were made, `last` the final error."""

    def __init__(self, attempts, last):
        super().__init__(f"gave up after {attempts} attempts: {last}")
        self.attempts = attempts
        self.last = last


def backoff_delays(attempts, base=0.5, cap=8.0):
    """The pause before each further try: `base`, doubled every time, never more than `cap`."""
    return [min(cap, base * 2**i) for i in range(attempts - 1)]


def call_with_retry(fn, attempts=3, base=0.5, cap=8.0, sleep=time.sleep, retry_on=(Exception,)):
    """Calls `fn()` and returns its result, trying up to `attempts` times in total when it raises."""
    if attempts < 1:
        raise ValueError("attempts must be at least 1")
    delays = backoff_delays(attempts, base, cap)
    for number in range(1, attempts + 1):
        try:
            return fn()
        except retry_on as error:
            if number == attempts:
                raise RetryError(attempts, error) from error
            sleep(delays[number - 1])
