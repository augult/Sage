THOUGHT = r'''Everything the README describes now holds: the sample report, the -n 3 report, stdin and
"-", the empty and all-skipped reports, usage errors with status 2, sorting and tie-breaks,
long endpoints, and the path normalisation examples. I'm done.'''

CONTENT = r'''`rollup.sh` is done. It is POSIX `sh` plus mawk and the usual text tools only:

- validates each line against the six-field grammar and counts the rest as skipped, ignoring empty lines;
- normalises paths (query, one trailing slash, numeric segments to `:id`) and groups by method + path;
- reports requests, skipped, window, and the slowest request (first in the file on a tie);
- computes `err%` and nearest-rank `p50`/`p95` from latencies sorted with `LC_ALL=C sort`;
- orders rows by count then endpoint bytes, prints the first N, and folds the remainder into `(other)`;
- reads a file, `-`, or stdin, rejects bad arguments with usage and status 2, and cleans up its temp dir.

`sh check.sh` passes for both `access.log` reports, and the edge cases above (empty log, all-skipped log, no trailing newline, `-n 007`, unreadable file, long endpoints, ties) behave as specified.'''

CALLS = []
