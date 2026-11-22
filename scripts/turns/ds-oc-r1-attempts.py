THOUGHT = r'''Migration is complete and verified: visible tests pass, no identifier occurrences remain,
and the CLI reports show-config, --help and FAILED messages as specified.'''

CONTENT = r'''Migration complete across the library, CLI, settings, manifests, scripts, config template and README, with `attempts = retries + 1` so try counts are unchanged.

- `fetchkit/retry.py`: `RetryError(attempts, last)` with message `gave up after N attempts: ...`, `backoff_delays(attempts)` produces one pause fewer than tries, `call_with_retry(fn, attempts=3)` rejects anything below 1.
- `fetchkit/client.py`: `DEFAULT_ATTEMPTS = 3`, `Client(attempts=...)`, `fetch(url, attempts=...)`, `budget_s = timeout * attempts`.
- `fetchkit/config.py`: `attempts` default 3, `FETCHKIT_ATTEMPTS`, zero or less is a `ConfigError`; unknown keys such as `retries` are rejected.
- `fetchkit/cli.py`: `--attempts` with default 3 in the help, `FAILED <url> after N attempts`.
- `fetchkit/jobs.py`: `PRIORITY_ATTEMPTS = {"low": 1, "normal": None, "high": 6}`, positive `attempts` only.
- Shipped settings moved with preserved behaviour: config template 5 attempts, nightly 5, smoke 1, backfill 8 with a 1-attempt probe, example manifest `attempts` values 2 and 1.

`python3 -m unittest discover -s tests` passes 7/7, `grep -rni retries` over the shipped files finds nothing, `show-config` prints `attempts = 3`, and a failing fetch prints `FAILED ... after 2 attempts` and exits 1.'''

CALLS = []
