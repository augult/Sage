"""The client: one transport, one retry policy."""

import time

from .retry import call_with_retry
from .transport import SimulatedTransport, TransportError

DEFAULT_RETRIES = 2


class Client:
    def __init__(self, transport=None, retries=DEFAULT_RETRIES, timeout=10.0, backoff=0.5, sleep=time.sleep):
        if retries < 0:
            raise ValueError("retries must not be negative")
        self.transport = transport if transport is not None else SimulatedTransport()
        self.retries = retries
        self.timeout = timeout
        self.backoff = backoff
        self.sleep = sleep

    @property
    def budget_s(self):
        """The longest one fetch can spend on the wire, in seconds, pauses not counted."""
        return self.timeout * (self.retries + 1)

    def fetch(self, url, retries=None):
        """The body of `url`. `retries` overrides the client's setting for this call."""
        retries = self.retries if retries is None else retries
        return call_with_retry(
            lambda: self.transport.get(url, self.timeout),
            retries=retries,
            base=self.backoff,
            sleep=self.sleep,
            retry_on=(TransportError,),
        )
