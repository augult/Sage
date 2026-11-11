"""Batch downloads described by a manifest.

    {"jobs": [{"url": "https://mirror.example/a.tar", "priority": "high", "retries": 1}]}

`priority` and `retries` are optional. A job's own `retries` wins over its priority, and a
priority without an entry in the table below uses the client's setting.
"""

import json

from .retry import RetryError

# Low-priority jobs are not worth waiting for; high-priority ones must get through.
PRIORITY_RETRIES = {"low": 0, "normal": None, "high": 5}

JOB_KEYS = {"url", "priority", "retries"}


class ManifestError(Exception):
    """The manifest cannot be used."""


class Job:
    def __init__(self, url, priority="normal", retries=None):
        if priority not in PRIORITY_RETRIES:
            raise ManifestError(f"unknown priority {priority!r} for {url}")
        if retries is not None and (not isinstance(retries, int) or retries < 0):
            raise ManifestError(f"retries must be a non-negative integer for {url}")
        self.url = url
        self.priority = priority
        self.retries = retries if retries is not None else PRIORITY_RETRIES[priority]


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
            client.fetch(job.url, retries=job.retries)
            ok = True
        except RetryError:
            ok = False
        results.append((job.url, client.transport.calls.get(job.url, 0) - before, ok))
    return results
