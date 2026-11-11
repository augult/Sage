"""The client: one transport, one policy for trying again."""

import time

from .retry import call_with_retry
from .transport import SimulatedTransport, TransportError

DEFAULT_ATTEMPTS = 3


class Client:
    def __init__(self, transport=None, attempts=DEFAULT_ATTEMPTS, timeout=10.0, backoff=0.5, sleep=time.sleep):
        if attempts < 1:
            raise ValueError("attempts must be at least 1")
        self.transport = transport if transport is not None else SimulatedTransport()
        self.attempts = attempts
        self.timeout = timeout
        self.backoff = backoff
        self.sleep = sleep

    @property
    def budget_s(self):
        """The longest one fetch can spend on the wire, in seconds, pauses not counted."""
        return self.timeout * self.attempts

    def fetch(self, url, attempts=None):
        """The body of `url`. `attempts` overrides the client's setting for this call."""
        attempts = self.attempts if attempts is None else attempts
        return call_with_retry(
            lambda: self.transport.get(url, self.timeout),
            attempts=attempts,
            base=self.backoff,
            sleep=self.sleep,
            retry_on=(TransportError,),
        )
