"""The wire. Real HTTP lives in the deployment package; this repository ships the simulated
transport that rehearsals and tests use, so nothing here needs a network.

Two environment variables drive it:

    FETCHKIT_FAIL_FIRST   how often a URL fails before it succeeds: a number for every URL
                          ("2"), or rules "text:n,text:n" where the first rule whose text occurs
                          in the URL decides and other URLs never fail
    FETCHKIT_TRACE        a file that gets one line per try: "<url> <number of the try>"
"""

import os


class TransportError(Exception):
    """One try failed."""


def _failures_for(url, spec):
    spec = (spec or "0").strip()
    if ":" not in spec:
        return int(spec)
    for rule in spec.split(","):
        text, _, count = rule.rpartition(":")
        if text.strip() in url:
            return int(count)
    return 0


class SimulatedTransport:
    def __init__(self, fail_first=None, trace=None):
        self.fail_first = os.environ.get("FETCHKIT_FAIL_FIRST") if fail_first is None else str(fail_first)
        self.trace = os.environ.get("FETCHKIT_TRACE") if trace is None else trace
        self.calls = {}

    def get(self, url, timeout):
        number = self.calls.get(url, 0) + 1
        self.calls[url] = number
        if self.trace:
            with open(self.trace, "a", encoding="utf-8") as out:
                out.write(f"{url} {number}\n")
        if number <= _failures_for(url, self.fail_first):
            raise TransportError(f"simulated failure {number} for {url}")
        return f"content of {url}\n".encode()
