THOUGHT = r'''Confirmed on disk: the earlier leftover scan raced the writes. All files now use attempts,
the tests pass, the CLI prints attempts = 3 and FAILED ... after 2 attempts, and no shipped
file mentions retries.'''

CONTENT = r'''Migration complete and verified. The earlier leftover scan raced the writes; a fresh read of the files shows the new names everywhere.

- Library: `RetryError(attempts, last)` with `gave up after N attempts`, `backoff_delays(attempts)` with one pause fewer than tries, `call_with_retry(fn, attempts=3)`, `DEFAULT_ATTEMPTS = 3`, `Client(attempts=...)`, `fetch(url, attempts=...)`, `budget_s = timeout * attempts`; below 1 raises `ValueError`.
- Settings: `attempts` defaults to 3, `FETCHKIT_ATTEMPTS` is honoured, unknown keys and values below 1 are `ConfigError`.
- CLI: `--attempts` with default 3 in its help, `show-config` prints `attempts = 3`, a failed download prints `FAILED <url> after N attempts`.
- Manifests: `PRIORITY_ATTEMPTS = {"low": 1, "normal": None, "high": 6}`, positive per-job `attempts` only, unknown key or zero is a `ManifestError`.
- Shipped behaviour preserved: template 5 attempts, nightly 5, smoke 1, backfill 8 with a 1-attempt probe, example manifest 2 and 1.

`python3 -m unittest discover -s tests` passes 7/7 and a case-insensitive scan of every shipped file finds no `retries`.'''

CALLS = []
