#!/bin/sh
# rollup.sh: per-endpoint summary of a gateway access log. See README.md.
LC_ALL=C
export LC_ALL

usage() {
    echo "usage: sh rollup.sh [-n N] [FILE]" >&2
    exit 2
}

top=10
if [ "${1-}" = "-n" ]; then
    [ $# -ge 2 ] || usage
    top=$2
    shift 2
fi
case "$top" in
    '' | *[!0-9]*) usage ;;
esac
[ "$top" -ge 1 ] || usage
[ $# -le 1 ] || usage
file=${1:--}
case "$file" in
    -?*) usage ;;
esac

tmp=$(mktemp -d) || exit 1
trap 'rm -rf "$tmp"' EXIT
tab=$(printf '\t')

# Pass 1: the valid requests as "endpoint, latency, error, bytes"; the summary lines go to a file.
awk -v summary="$tmp/summary" '
BEGIN { OFS = "\t" }
$0 == "" { next }
NF != 6 || $2 !~ /^(GET|POST|PUT|PATCH|DELETE)$/ || $3 !~ /^\// || $4 !~ /^[0-9][0-9][0-9]$/ || $5 !~ /^[0-9]+ms$/ || $6 !~ /^[0-9]+b$/ {
    skipped++
    next
}
{
    requests++
    stamp = $1 ""
    if (requests == 1 || stamp < first) first = stamp
    if (requests == 1 || stamp > last) last = stamp
    latency = $5 + 0
    if (requests == 1 || latency > worst) {
        worst = latency
        slowest = latency "ms " $2 " " $3 " at " stamp
    }
    path = $3
    query = index(path, "?")
    if (query) path = substr(path, 1, query - 1)
    if (length(path) > 1 && substr(path, length(path)) == "/") path = substr(path, 1, length(path) - 1)
    n = split(path, segment, "/")
    normal = ""
    for (i = 2; i <= n; i++) normal = normal "/" (segment[i] ~ /^[0-9]+$/ ? ":id" : segment[i])
    print $2 " " normal, latency, ($4 >= 500 ? 1 : 0), $6 + 0
}
END {
    print "requests: " (requests + 0) > summary
    print "skipped: " (skipped + 0) > summary
    if (requests) {
        print "window: " first " .. " last > summary
        print "slowest: " slowest > summary
    } else {
        print "window: -" > summary
    }
}' "$file" > "$tmp/requests" || exit 1

cat "$tmp/summary"
[ -s "$tmp/requests" ] || exit 0
echo

# Pass 2: one line per endpoint (latencies arrive sorted). Pass 3: the table.
sort -t "$tab" -k1,1 -k2,2n "$tmp/requests" | awk '
BEGIN { FS = OFS = "\t" }
function rank(p) { return latency[int((p * count + 99) / 100)] }
function flush() {
    if (count) print count, endpoint, errors, rank(50), rank(95), bytes
    count = errors = bytes = 0
}
$1 != endpoint { flush(); endpoint = $1 }
{ latency[++count] = $2; errors += $3; bytes += $4 }
END { flush() }' | sort -t "$tab" -k1,1nr -k2,2 | awk -v top="$top" '
BEGIN {
    FS = "\t"
    row = "%-32s %7s %6s %6s %6s %10s\n"
    printf row, "endpoint", "count", "err%", "p50", "p95", "bytes"
}
NR <= top { printf row, $2, $1, sprintf("%.1f", $3 * 100 / $1), $4, $5, $6; next }
{ count += $1; errors += $3; bytes += $6 }
END { if (count) printf row, "(other)", count, sprintf("%.1f", errors * 100 / count), "-", "-", bytes }'
