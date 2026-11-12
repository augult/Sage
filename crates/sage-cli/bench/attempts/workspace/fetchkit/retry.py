"""Calling something again when it fails."""

import time


class RetryError(Exception):
    """Every try failed. `retries` is how many retries were made, `last` the final error."""

    def __init__(self, retries, last):
        super().__init__(f"gave up after {retries} retries: {last}")
        self.retries = retries
        self.last = last


def backoff_delays(retries, base=0.5, cap=8.0):
    """The pause before each retry: `base`, doubled every time, never more than `cap`."""
    return [min(cap, base * 2**i) for i in range(retries)]


def call_with_retry(fn, retries=2, base=0.5, cap=8.0, sleep=time.sleep, retry_on=(Exception,)):
    """Calls `fn()` and returns its result, retrying up to `retries` times when it raises."""
    if retries < 0:
        raise ValueError("retries must not be negative")
    delays = backoff_delays(retries, base, cap)
    for attempt in range(retries + 1):
        try:
            return fn()
        except retry_on as error:
            if attempt == retries:
                raise RetryError(retries, error) from error
            sleep(delays[attempt])
