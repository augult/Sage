THOUGHT = r'''Both sample reports match and every contract case I tried behaves as the README says:
stdin/"-", empty and all-skipped logs, bad arguments exiting 2, normalisation, ties in byte
order, long endpoints, N above the endpoint count, and a last line without a newline. Nothing
left to change.'''

CONTENT = r'''`rollup.sh` is implemented and passes `sh check.sh` for both expected reports.

It uses only POSIX `sh`, mawk and the usual text tools: one pass validates the six fields and counts skipped lines, normalises paths and groups them by method, a sorted-latency pass picks the nearest-rank `p50`/`p95`, and the rows are ordered by count then endpoint bytes with the remainder folded into `(other)`. It reads a file, `-` or stdin, rejects bad arguments with usage and status 2, and cleans up its temp directory.

Beyond the sample I checked stdin, an empty and an all-skipped log, unreadable files, the normalisation examples, count ties, an over-long endpoint, `-n` larger than the endpoint count, `-n 007` and a log without a trailing newline - all match the spec.'''

CALLS = []
