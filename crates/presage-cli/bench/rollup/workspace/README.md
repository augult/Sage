# rollup

`rollup.sh` turns an access log of the gateway into the per-endpoint summary that goes into the
morning mail. It runs on the log hosts, which have a POSIX `sh`, `awk` (it is **mawk**, so no gawk
extensions such as `asort`, `gensub` or `PROCINFO`) and the usual `sort`, `sed`, `tr`, `cut` and
`head`. There is no Python, Perl or Node there, and nothing can be installed.

    sh rollup.sh [-n N] [FILE]

`FILE` is the log; without it, or when it is `-`, the log is read from standard input. `-n N`
sets how many endpoints are listed (default 10; N is a positive integer). Anything else on the
command line prints a usage line to standard error and exits with status 2.

`sh check.sh` compares the output for `access.log` with `expected.txt` and `expected_top3.txt`.

## The log

One request per line, six fields separated by single spaces:

    2026-03-02T10:15:07Z GET /api/users/42?verbose=1 200 13ms 512b

timestamp (UTC, always in this form), method, path with an optional query string, status,
latency, response size.

A line is **valid** if it has exactly six fields, the method is one of `GET`, `POST`, `PUT`,
`PATCH`, `DELETE`, the path starts with `/`, the status is exactly three digits, the latency is
digits followed by `ms` and the size is digits followed by `b`. Empty lines are ignored. Every
other line is **skipped**: it is counted, and otherwise plays no part.

The lines of a log are not necessarily in order of time.

## Endpoints

Requests are grouped by endpoint: the method, a space, and the normalised path. To normalise a
path,

1. drop the query string (everything from the first `?`);
2. drop one trailing `/`, unless the path is just `/`;
3. replace every segment that consists only of digits by `:id`.

So `GET /api/users/42/?verbose=1` belongs to `GET /api/users/:id`, while `/v2/users` and
`/users/42a` stay as they are.

## The report

    requests: 61
    skipped: 2
    window: 2026-03-02T10:15:02Z .. 2026-03-02T10:19:49Z
    slowest: 265ms POST /api/orders at 2026-03-02T10:18:06Z

    endpoint                           count   err%    p50    p95      bytes
    GET /api/users/:id                    17    0.0     35     95      82308
    GET /api/search                       12    0.0     39    124      40861
    POST /api/orders                      10    0.0     43    265      62860
    (other)                               22    9.1      -      -     106155

(This is `sh rollup.sh -n 3 access.log`.)

- `requests` is the number of valid lines, `skipped` the number of skipped ones.
- `window` is the earliest and the latest timestamp among the valid lines.
- `slowest` is the valid request with the highest latency, with its path as it is in the log.
  If several share the highest latency, it is the one that comes first in the file.
- Then an empty line, the header, and one row per endpoint.

The columns of a row:

- `count`: the endpoint's requests.
- `err%`: the share of them with a status of 500 or above, in percent, printed with `%.1f`.
- `p50` and `p95`: the 50th and 95th percentile of the latency in ms, by the nearest-rank
  method: with the latencies sorted in ascending order, the percentile `p` is the value at
  position `ceil(p / 100 * count)`, counting from 1.
- `bytes`: the sum of the response sizes.

Rows are sorted by `count`, highest first; endpoints with the same count are in ascending byte
order of the endpoint (what `LC_ALL=C sort` does). Only the first N rows are listed. If there are
more endpoints, one last row `(other)` sums up the rest: its `count`, `err%` and `bytes` are
computed over all the remaining requests, and its percentiles are `-`.

The header and every row are printed with the format `%-32s %7s %6s %6s %6s %10s`. An endpoint
longer than 32 characters is printed in full and pushes its row to the right.

When there is no valid line at all, the report is just

    requests: 0
    skipped: 3
    window: -

without `slowest`, the empty line and the table.
