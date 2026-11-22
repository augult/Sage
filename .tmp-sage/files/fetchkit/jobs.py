"""Batch downloads described by a manifest.

    {"jobs": [{"url": "https://mirror.example/a.tar", "priority": "high", "attempts": 1}]}

`priority` and `attempts` are optional. A job's own `attempts` wins over its priority, and a
priority without an entry in the table below uses the client's setting.
"""

import json

from .retry import RetryError

# Low-priority jobs are not worth waiting for; high-priority ones must get through.
PRIORITY_ATTEMPTS = {"low": 1, "normal": None, "high": 6}

JOB_KEYS = {"url", "priority", "attempts"}


class ManifestError(Exception):
    """The manifest cannot be used."""


class Job:
    def __init__(self, url, priority="normal", attempts=None):
        if priority not in PRIORITY_ATTEMPTS:
            raise ManifestError(f"unknown priority {priority!r} for {url}")
        if attempts is not None and (not isinstance(attempts, int) or attempts < 1):
            raise ManifestError(f"attempts must be a positive integer for {url}")
        self.url = url
        self.priority = priority
        self.attempts = attempts if attempts is not None else PRIORITY_ATTEMPTS[priority]


def load_manifest(path):
    with open(path, encoding="utf-8") as source:
        try:
            document = json.load(source)
        except ValueError as error:
            raise ManifestError(f"{path} is not JSON: {error}") from None
    jobs = []
    for entry in document.get("jobs", []):
        unknown = set(entry) - JOB_KEYS
        if unknown:
            raise ManifestError(f"unknown key {sorted(unknown)[0]!r} in a job of {path}")
        if "url" not in entry:
            raise ManifestError(f"a job of {path} has no url")
        jobs.append(Job(**entry))
    return jobs


def run_jobs(jobs, client):
    """Fetches every job. Returns `(url, tries, ok)` for each, in order."""
    results = []
    for job in jobs:
        before = client.transport.calls.get(job.url, 0)
        try:
            client.fetch(job.url, attempts=job.attempts)
            ok = True
        except RetryError:
            ok = False
        results.append((job.url, client.transport.calls.get(job.url, 0) - before, ok))
    return results
