# fetchkit

Downloads that try again. A library (`fetchkit.Client`), a command line (`python3 -m fetchkit`)
and a few operational scripts around them.

## Library

```python
from fetchkit import Client

client = Client(attempts=4, timeout=15)      # up to four tries per download
body = client.fetch("https://mirror.example/packages/core.tar")
key = client.fetch("https://mirror.example/keys/release.asc", attempts=1)  # this one only once
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

| setting   | default | meaning                                              |
|-----------|---------|------------------------------------------------------|
| `attempts`| 3       | how often a download is tried in all                 |
| `timeout` | 10      | seconds before one try is abandoned                  |
| `backoff` | 0.5     | seconds before the first retry, doubled each time    |

They come from, in rising order of precedence: the defaults, the `[fetch]` section of the config
file (`config/fetchkit.ini.template` is a starting point), the environment (`FETCHKIT_ATTEMPTS`,
`FETCHKIT_TIMEOUT`, `FETCHKIT_BACKOFF`) and the command-line flags. An option the config file
does not know is an error.

## Manifests

`--manifest` takes a JSON file of jobs, see `examples/manifest.json`. A job may set its own
`attempts`, or a `priority` (`low`: tried once, `high`: tried up to six times, `normal`: the
client's setting).

## Scripts

- `scripts/nightly.sh`: the nightly mirror refresh, with five attempts per download.
- `scripts/smoke.sh`: the deploy job's smoke check, a single attempt.
- `scripts/backfill.py`: one-off backfill of the yearly archives.

## Rehearsing without a network

The transport in this repository is simulated. `FETCHKIT_FAIL_FIRST=2` makes every URL fail twice
before it succeeds, and `FETCHKIT_TRACE=tries.log` records every try. See `fetchkit/transport.py`.

## Tests

    python3 -m unittest discover -s tests
