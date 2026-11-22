THOUGHT = r'''Mechanical but behaviour-preserving: attempts = retries + 1 everywhere. I will move the
library, CLI, settings, manifests, scripts, docs and template in one pass, then verify the edge
cases the tests omit (zero rejected by each layer, old names rejected everywhere).'''

CONTENT = r'''I'll carry the `retries` -> `attempts` migration through the library, CLI, settings, manifests, scripts, README and config template, keeping `attempts = retries + 1` so nothing changes how often a download is tried.'''

CALLS = [
    {"name": "write", "arguments": {"path": "fetchkit/retry.py", "overwrite": True, "content": r'''"""Calling something again when it fails."""

import time


class RetryError(Exception):
    """Every try failed. `attempts` is how many tries were made, `last` the final error."""

    def __init__(self, attempts, last):
        super().__init__(f"gave up after {attempts} attempts: {last}")
        self.attempts = attempts
        self.last = last


def backoff_delays(attempts, base=0.5, cap=8.0):
    """The pause before each retry: `base`, doubled every time, never more than `cap`."""
    return [min(cap, base * 2**i) for i in range(attempts - 1)]


def call_with_retry(fn, attempts=3, base=0.5, cap=8.0, sleep=time.sleep, retry_on=(Exception,)):
    """Calls `fn()` and returns its result, trying up to `attempts` times when it raises."""
    if attempts < 1:
        raise ValueError("attempts must be at least 1")
    delays = backoff_delays(attempts, base, cap)
    for attempt in range(attempts):
        try:
            return fn()
        except retry_on as error:
            if attempt == attempts - 1:
                raise RetryError(attempts, error) from error
            sleep(delays[attempt])
'''}},
    {"name": "write", "arguments": {"path": "fetchkit/client.py", "overwrite": True, "content": r'''"""The client: one transport, one attempt policy."""

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
'''}},
    {"name": "write", "arguments": {"path": "fetchkit/config.py", "overwrite": True, "content": r'''"""Settings: defaults, then the config file, then the environment."""

import configparser
import os

DEFAULTS = {"attempts": 3, "timeout": 10.0, "backoff": 0.5}


class ConfigError(Exception):
    """A setting that cannot be used."""


def _convert(key, raw):
    kind = type(DEFAULTS[key])
    try:
        return kind(raw)
    except (TypeError, ValueError):
        raise ConfigError(f"{key} must be {kind.__name__}: {raw!r}") from None


def validate(config):
    if config["attempts"] < 1:
        raise ConfigError("attempts must be at least 1")
    if config["timeout"] <= 0:
        raise ConfigError("timeout must be positive")
    if config["backoff"] < 0:
        raise ConfigError("backoff must not be negative")
    return config


def load_config(path=None, env=None):
    """The effective settings. `path` is an INI file with a `[fetch]` section, or None."""
    env = os.environ if env is None else env
    config = dict(DEFAULTS)
    if path is not None:
        parser = configparser.ConfigParser(inline_comment_prefixes=(";", "#"))
        if not parser.read(path, encoding="utf-8"):
            raise ConfigError(f"cannot read {path}")
        if parser.has_section("fetch"):
            for key, raw in parser.items("fetch"):
                if key not in DEFAULTS:
                    raise ConfigError(f"unknown option {key!r} in {path}")
                config[key] = _convert(key, raw)
    # Every setting can be overridden as FETCHKIT_<NAME>.
    for key in DEFAULTS:
        raw = env.get("FETCHKIT_" + key.upper())
        if raw is not None:
            config[key] = _convert(key, raw)
    return validate(config)
'''}},
    {"name": "write", "arguments": {"path": "fetchkit/jobs.py", "overwrite": True, "content": r'''"""Batch downloads described by a manifest.

    {"jobs": [{"url": "https://mirror.example/a.tar", "priority": "high", "attempts": 2}]}

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
            raise ManifestError(f"attempts must be an integer of at least 1 for {url}")
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
'''}},
    {"name": "write", "arguments": {"path": "fetchkit/__init__.py", "overwrite": True, "content": r'''"""fetchkit: downloads that try again."""

from .client import DEFAULT_ATTEMPTS, Client
from .retry import RetryError, backoff_delays, call_with_retry

__all__ = ["DEFAULT_ATTEMPTS", "Client", "RetryError", "backoff_delays", "call_with_retry"]
'''}},
    {"name": "write", "arguments": {"path": "fetchkit/cli.py", "overwrite": True, "content": r'''"""`python3 -m fetchkit`: fetch URLs or a manifest, or show the effective settings."""

import argparse
import sys

from .client import Client
from .config import ConfigError, load_config, validate
from .jobs import Job, ManifestError, load_manifest, run_jobs


def build_parser():
    parser = argparse.ArgumentParser(prog="fetchkit", description="Download files, retrying the ones that fail.")
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--config", metavar="PATH", help="settings file (INI, section [fetch])")
    common.add_argument("--attempts", type=int, metavar="N", help="try a download up to N times, the first try included (default: 3)")
    common.add_argument("--timeout", type=float, metavar="SECONDS", help="give up on one try after this long (default: 10)")
    commands = parser.add_subparsers(dest="command", required=True)
    fetch = commands.add_parser("fetch", parents=[common], help="download URLs")
    fetch.add_argument("--manifest", metavar="FILE", help="a JSON manifest of jobs")
    fetch.add_argument("urls", nargs="*", metavar="URL")
    commands.add_parser("show-config", parents=[common], help="print the effective settings")
    return parser


def effective_config(args):
    """Flags win over the environment, which wins over the config file."""
    config = load_config(args.config)
    if args.attempts is not None:
        config["attempts"] = args.attempts
    if args.timeout is not None:
        config["timeout"] = args.timeout
    return validate(config)


def main(argv=None):
    args = build_parser().parse_args(argv)
    try:
        config = effective_config(args)
        if args.command == "show-config":
            for key in sorted(config):
                print(f"{key} = {config[key]}")
            return 0
        jobs = [Job(url) for url in args.urls]
        if args.manifest:
            jobs += load_manifest(args.manifest)
    except (ConfigError, ManifestError) as error:
        print(f"fetchkit: {error}", file=sys.stderr)
        return 2
    if not jobs:
        print("fetchkit: nothing to fetch", file=sys.stderr)
        return 2
    client = Client(attempts=config["attempts"], timeout=config["timeout"], backoff=config["backoff"], sleep=lambda _: None)
    failed = 0
    for url, tries, ok in run_jobs(jobs, client):
        if ok:
            print(f"ok {url} ({tries} tries)")
        else:
            failed += 1
            print(f"FAILED {url} after {tries} attempts")
    return 1 if failed else 0
'''}},
    {"name": "write", "arguments": {"path": "config/fetchkit.ini.template", "overwrite": True, "content": r'''; Copy to ~/.config/fetchkit.ini and adjust.
; The mirrors behind the office proxy drop connections, hence more patience than the default.

[fetch]
; How many times a download is tried, the first try included, before fetchkit gives up on it.
attempts = 5
; Seconds before one try is abandoned.
timeout = 20
; Seconds before the first retry; doubled before each further one.
backoff = 0.5
'''}},
    {"name": "write", "arguments": {"path": "examples/manifest.json", "overwrite": True, "content": r'''{
  "jobs": [
    {"url": "https://mirror.example/index.json", "priority": "high"},
    {"url": "https://mirror.example/packages/core.tar"},
    {"url": "https://mirror.example/packages/extras.tar", "priority": "low"},
    {"url": "https://mirror.example/packages/docs.tar", "priority": "low", "attempts": 2},
    {"url": "https://mirror.example/keys/release.asc", "priority": "high", "attempts": 1}
  ]
}
'''}},
    {"name": "write", "arguments": {"path": "scripts/nightly.sh", "overwrite": True, "content": r'''#!/bin/sh
# Nightly refresh of the mirror. The upstream is flaky at night, so every download gets five
# attempts instead of the default three. Extra arguments are passed on to fetchkit.
set -eu
cd "$(dirname "$0")/.."
exec python3 -m fetchkit fetch --attempts 5 --manifest examples/manifest.json "$@"
'''}},
    {"name": "write", "arguments": {"path": "scripts/smoke.sh", "overwrite": True, "content": r'''#!/bin/sh
# Smoke check run by the deploy job: is the index reachable right now? One attempt, no patience,
# so that a dead upstream fails the deploy in seconds.
set -eu
cd "$(dirname "$0")/.."
FETCHKIT_ATTEMPTS=1 FETCHKIT_TIMEOUT=3 exec python3 -m fetchkit fetch https://mirror.example/index.json
'''}},
    {"name": "write", "arguments": {"path": "scripts/backfill.py", "overwrite": True, "content": r'''#!/usr/bin/env python3
"""One-off backfill of the archive mirror: `python3 scripts/backfill.py 2019 2020 ...`.

Probes the index once (no point in starting if it is down), then fetches one archive per year
with a lot of patience, because the archive host throttles.
"""

import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from fetchkit import Client, RetryError

INDEX = "https://mirror.example/index.json"
PROBE = {"timeout": 2.0}
PATIENT = {"attempts": 8, "timeout": 30.0}


def main(years):
    quiet = {"backoff": 0.0, "sleep": lambda _: None}
    probe = Client(**PROBE, **quiet)
    try:
        probe.fetch(INDEX, attempts=1)
    except RetryError as error:
        print(f"index unreachable, not starting: {error}")
        return 1
    client = Client(**PATIENT, **quiet)
    missing = []
    for year in years:
        url = f"https://mirror.example/archive/{year}.tar"
        try:
            client.fetch(url)
            print(f"fetched {url}")
        except RetryError as error:
            print(f"skipped {url}: {error}")
            missing.append(year)
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
'''}},
    {"name": "write", "arguments": {"path": "README.md", "overwrite": True, "content": r'''# fetchkit

Downloads that try again. A library (`fetchkit.Client`), a command line (`python3 -m fetchkit`)
and a few operational scripts around them.

## Library

```python
from fetchkit import Client

client = Client(attempts=4, timeout=15)      # up to four tries per download
body = client.fetch("https://mirror.example/packages/core.tar")
key = client.fetch("https://mirror.example/keys/release.asc", attempts=1)   # this one only once
```

`Client.fetch` raises `fetchkit.RetryError` when the last try failed too; its `attempts` attribute
says how many tries were made. `client.budget_s` is the longest a single fetch can spend on the
wire.

## Command line

```
python3 -m fetchkit fetch [--config PATH] [--attempts N] [--timeout SECONDS] [--manifest FILE] [URL ...]
python3 -m fetchkit show-config [--config PATH] [--attempts N] [--timeout SECONDS]
```

```
$ python3 -m fetchkit fetch --attempts 2 https://mirror.example/packages/core.tar
ok https://mirror.example/packages/core.tar (1 tries)
```

That is two attempts: the download is tried twice at most. The exit status is 1 when a download
failed and 2 when the settings or the manifest are unusable.

## Settings

| setting    | default | meaning                                            |
|------------|---------|----------------------------------------------------|
| `attempts` | 3       | how many tries a download gets, the first included |
| `timeout`  | 10      | seconds before one try is abandoned                |
| `backoff`  | 0.5     | seconds before the first retry, doubled each time  |

They come from, in rising order of precedence: the defaults, the `[fetch]` section of the config
file (`config/fetchkit.ini.template` is a starting point), the environment (`FETCHKIT_ATTEMPTS`,
`FETCHKIT_TIMEOUT`, `FETCHKIT_BACKOFF`) and the command-line flags. An option the config file
does not know is an error.

## Manifests

`--manifest` takes a JSON file of jobs, see `examples/manifest.json`. A job may set its own
`attempts`, or a `priority` (`low`: one attempt, `high`: six attempts, `normal`: the client's
setting).

## Scripts

- `scripts/nightly.sh`: the nightly mirror refresh, with five attempts per download.
- `scripts/smoke.sh`: the deploy job's smoke check, one attempt.
- `scripts/backfill.py`: one-off backfill of the yearly archives.

## Rehearsing without a network

The transport in this repository is simulated. `FETCHKIT_FAIL_FIRST=2` makes every URL fail twice
before it succeeds, and `FETCHKIT_TRACE=tries.log` records every try. See `fetchkit/transport.py`.

## Tests

    python3 -m unittest discover -s tests
'''}},
    {"name": "shell", "arguments": {"expect_exit": "0", "timeout": 60, "cmd": r'''python3 - <<'PY'
import json, os, sys, tempfile
sys.path.insert(0, ".")
from fetchkit import Client, DEFAULT_ATTEMPTS, RetryError, backoff_delays, call_with_retry
from fetchkit.config import ConfigError, load_config
from fetchkit.jobs import Job, load_manifest, ManifestError

assert DEFAULT_ATTEMPTS == 3
for bad in (0, -1):
    for label, fn, err in (
        ("Client", lambda: Client(attempts=bad), ValueError),
        ("call_with_retry", lambda: call_with_retry(lambda: None, attempts=bad), ValueError),
        ("Job", lambda: Job("u", attempts=bad), ManifestError),
        ("settings", lambda: load_config(env={"FETCHKIT_ATTEMPTS": str(bad)}), ConfigError),
    ):
        try:
            fn()
        except err:
            pass
        else:
            raise SystemExit(f"{label} accepted {bad}")
for label, fn in (
    ("Client(retries=)", lambda: Client(retries=1)),
    ("Job(retries=)", lambda: Job("u", retries=1)),
):
    try:
        fn()
    except TypeError:
        pass
    else:
        raise SystemExit(f"{label} still accepted")
d = tempfile.mkdtemp()
m = os.path.join(d, "m.json")
open(m, "w").write(json.dumps({"jobs": [{"url": "u", "retries": 1}]}))
try:
    load_manifest(m)
except ManifestError:
    pass
else:
    raise SystemExit("manifest accepted retries")
c = os.path.join(d, "c.ini")
open(c, "w").write("[fetch]\nretries = 4\n")
try:
    load_config(c)
except ConfigError:
    pass
else:
    raise SystemExit("config accepted retries")
assert backoff_delays(1) == []
assert backoff_delays(3) == [0.5, 1.0]
assert backoff_delays(4, base=1.0) == [1.0, 2.0, 4.0]
e = RetryError(3, "boom")
assert e.attempts == 3 and str(e) == "gave up after 3 attempts: boom"
print("EDGE OK")
PY
python3 -m fetchkit fetch --retries 1 https://mirror.example/a 2>/dev/null; [ $? -eq 2 ]
python3 -m fetchkit show-config --attempts 2 | grep -qx "attempts = 2"
! rg -n -w retries --glob '!docs/**' --glob '!tests/**' --glob '!fetchkit/transport.py' .'''}},
    {"name": "finish", "arguments": {"summary": "Migrated fetchkit from `retries` to `attempts` throughout, keeping behaviour identical (attempts = retries + 1). Library: DEFAULT_ATTEMPTS = 3, Client(attempts=), fetch(url, attempts=), call_with_retry(fn, attempts=3), backoff_delays(attempts) yielding attempts-1 pauses, RetryError.attempts with the message `gave up after N attempts: ...`. Zero or less is rejected as ValueError (library), ConfigError (settings) and ManifestError (manifest). The CLI flag is --attempts and a failed download prints `FAILED <url> after N attempts`; jobs.PRIORITY_ATTEMPTS maps low/normal/high to 1/None/6. Config defaults, FETCHKIT_ATTEMPTS, the manifest key, the scripts (nightly 5 attempts, smoke 1, backfill patient 8 and probe 1), the config template, the example manifest and the README all moved with it, so old arguments, flags, config keys and manifest keys are rejected. tests/, docs/attempts-migration.md and fetchkit/transport.py were not touched.", "unverified": "Only the sandbox's watched unittest run and the chained shell check executed; nothing ran against a real network transport. The shell check covers zero/negative rejection in every layer, rejection of the old names, the backoff counts and the error message, and that no `retries` identifier remains outside docs/, tests/ and transport.py."}}
]
