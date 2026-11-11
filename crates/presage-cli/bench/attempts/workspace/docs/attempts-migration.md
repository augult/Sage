# From `retries` to `attempts`

Status: agreed, tests updated, implementation outstanding.

## Why

"Retries" keeps being read as "tries". Twice this year somebody set `retries = 1` meaning "try
once". We are switching to the number that people actually think in: `attempts`, the total number
of tries, the first one included.

## What changes

`attempts = retries + 1`, everywhere. Nothing may change how often anything is actually tried:
a place that allowed four retries allows five attempts afterwards.

| before                              | after                                 |
|-------------------------------------|---------------------------------------|
| `Client(retries=2)` (the default)   | `Client(attempts=3)` (the default)    |
| `client.fetch(url, retries=0)`      | `client.fetch(url, attempts=1)`       |
| `call_with_retry(fn, retries=2)`    | `call_with_retry(fn, attempts=3)`     |
| `backoff_delays(retries)`           | `backoff_delays(attempts)`: one pause fewer than there are attempts |
| `RetryError.retries`                | `RetryError.attempts`: the tries that were made |
| `fetchkit.DEFAULT_RETRIES` (2)      | `fetchkit.DEFAULT_ATTEMPTS` (3)       |
| `--retries N`                       | `--attempts N`                        |
| `retries = N` in the config file    | `attempts = N`                        |
| `FETCHKIT_RETRIES`                  | `FETCHKIT_ATTEMPTS`                   |
| `"retries": N` in a manifest job    | `"attempts": N`                       |
| `jobs.PRIORITY_RETRIES`             | `jobs.PRIORITY_ATTEMPTS`              |

Details:

- The smallest valid value is 1. Zero or less is rejected the way a negative `retries` was:
  `ValueError` from the library, `ConfigError` for settings, `ManifestError` in a manifest.
- No compatibility layer. The old names are simply gone: `Client(retries=1)` is a `TypeError`,
  `--retries` is an unknown flag, and a config file or manifest that still says `retries` is
  rejected like any other unknown key.
- `RetryError`'s message becomes `gave up after N attempts: <last error>`, and the command line
  reports a failed download as `FAILED <url> after N attempts`, N being the tries made.
- Defaults keep their effect: where the code, the help text or the docs say "2 retries" they say
  "3 attempts" afterwards.
- Everything that ships in this repository moves with the library: the config template, the
  scripts, the example manifest, the README. Their behaviour stays what it is today.
- When you are done, `retries` no longer occurs as a parameter, attribute, constant, option, flag,
  key or environment variable anywhere outside this note. Prose may of course still say that
  something is retried.
