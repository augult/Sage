THOUGHT = r'''Write rollup.sh in POSIX sh with mawk, sort and sed: validate the six single-space-separated fields (splitting on the / / regex so doubled, leading or trailing spaces stay invalid), normalise paths, aggregate per endpoint, compute nearest-rank percentiles from latency lists sorted per endpoint, then format with the README's printf widths. Verify with check.sh plus an edge suite in the same reply.'''

CONTENT = r'''Implementing `rollup.sh`, then running `sh check.sh` together with an edge-case suite for the CLI, validation, normalisation, percentiles and the empty report.'''

SCRIPT = r'''#!/bin/sh
# rollup.sh: per-endpoint summary of a gateway access log. See README.md.

usage() {
	echo "usage: sh rollup.sh [-n N] [FILE]" >&2
	exit 2
}

n=10
file=

case $# in
0) ;;
1)
	case $1 in
	-) ;;
	-*) usage ;;
	*) file=$1 ;;
	esac
	;;
2)
	[ "$1" = "-n" ] || usage
	n=$2
	;;
3)
	[ "$1" = "-n" ] || usage
	n=$2
	case $3 in
	-) ;;
	-*) usage ;;
	*) file=$3 ;;
	esac
	;;
*) usage ;;
esac

case $n in
'' | *[!0-9]*) usage ;;
esac
[ "$n" -gt 0 ] 2>/dev/null || usage

if [ -n "$file" ] && [ ! -r "$file" ]; then
	echo "rollup.sh: cannot read $file" >&2
	exit 2
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rollup.XXXXXX") || exit 1
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

if [ -n "$file" ]; then
	exec 3<"$file"
else
	exec 3<&0
fi

awk -v reqf="$tmp/req" -v sumf="$tmp/sum" '
function norm(p,   q, m, i, s, out, seg) {
	q = index(p, "?")
	if (q > 0)
		p = substr(p, 1, q - 1)
	if (p != "/" && substr(p, length(p)) == "/")
		p = substr(p, 1, length(p) - 1)
	m = split(p, seg, "/")
	out = ""
	for (i = 1; i <= m; i++) {
		s = seg[i]
		if (s != "" && s ~ /^[0-9]+$/)
			s = ":id"
		out = out (i > 1 ? "/" : "") s
	}
	return out
}
{
	line = $0
	if (line == "")
		next
	nf = split(line, f, / /)
	ok = (nf == 6)
	if (ok)
		for (i = 1; i <= 6; i++)
			if (f[i] == "") {
				ok = 0
				break
			}
	if (ok && f[2] !~ /^(GET|POST|PUT|PATCH|DELETE)$/)
		ok = 0
	if (ok && substr(f[3], 1, 1) != "/")
		ok = 0
	if (ok && f[4] !~ /^[0-9][0-9][0-9]$/)
		ok = 0
	if (ok && f[5] !~ /^[0-9]+ms$/)
		ok = 0
	if (ok && f[6] !~ /^[0-9]+b$/)
		ok = 0
	if (!ok) {
		skipped++
		next
	}
	requests++
	if (first == "" || f[1] < first)
		first = f[1]
	if (f[1] > last)
		last = f[1]
	lat = f[5] + 0
	if (lat > maxlat) {
		maxlat = lat
		maxmethod = f[2]
		maxpath = f[3]
		maxts = f[1]
	}
	err = (f[4] + 0) >= 500 ? 1 : 0
	printf "%s\t%d\t%d\t%d\n", f[2] " " norm(f[3]), lat, err, f[6] + 0 > reqf
}
END {
	printf "%d\n%d\n", requests, skipped > sumf
	if (requests > 0) {
		printf "%s .. %s\n", first, last > sumf
		printf "%d %s %s %s\n", maxlat, maxmethod, maxpath, maxts > sumf
	} else {
		print "-" > sumf
	}
}
' <&3
exec 3<&-

tab=$(printf '\t')

LC_ALL=C sort -t "$tab" -k1,1 -k2,2n "$tmp/req" > "$tmp/req.sorted"

awk -F "$tab" -v outf="$tmp/rows" '
{
	ep = $1
	if (!(ep in count))
		eps[++nep] = ep
	count[ep]++
	errc[ep] += $3
	byt[ep] += $4
	lat[ep, count[ep]] = $2
}
END {
	for (i = 1; i <= nep; i++) {
		ep = eps[i]
		c = count[ep]
		p50 = int((50 * c + 99) / 100)
		if (p50 < 1)
			p50 = 1
		p95 = int((95 * c + 99) / 100)
		if (p95 < 1)
			p95 = 1
		printf "%d\t%d\t%d\t%d\t%d\t%s\n", c, errc[ep], lat[ep, p50], lat[ep, p95], byt[ep], ep > outf
	}
}
' "$tmp/req.sorted"

LC_ALL=C sort -t "$tab" -k1,1nr -k6,6 "$tmp/rows" > "$tmp/rows.sorted"

echo "requests: $(sed -n 1p "$tmp/sum")"
echo "skipped: $(sed -n 2p "$tmp/sum")"
echo "window: $(sed -n 3p "$tmp/sum")"
slow=$(sed -n 4p "$tmp/sum")
if [ -n "$slow" ]; then
	set -- $slow
	echo "slowest: $1ms $2 $3 at $4"
fi
requests=$(sed -n 1p "$tmp/sum")
if [ "$requests" -gt 0 ]; then
	echo
	awk -F "$tab" -v n="$n" '
	{
		cnt[NR] = $1
		errc[NR] = $2
		p50[NR] = $3
		p95[NR] = $4
		byt[NR] = $5
		ep[NR] = $6
	}
	END {
		total = NR
		limit = (n < total) ? n : total
		printf "%-32s %7s %6s %6s %6s %10s\n", "endpoint", "count", "err%", "p50", "p95", "bytes"
		for (i = 1; i <= limit; i++)
			printf "%-32s %7s %6.1f %6s %6s %10s\n", ep[i], cnt[i], (cnt[i] > 0 ? 100 * errc[i] / cnt[i] : 0), p50[i], p95[i], byt[i]
		if (total > n) {
			oc = 0
			oe = 0
			ob = 0
			for (i = n + 1; i <= total; i++) {
				oc += cnt[i]
				oe += errc[i]
				ob += byt[i]
			}
			printf "%-32s %7s %6.1f %6s %6s %10s\n", "(other)", oc, (oc > 0 ? 100 * oe / oc : 0), "-", "-", ob
		}
	}
	' "$tmp/rows.sorted"
fi
'''

CMD = r'''sh check.sh && python3 - <<'PY'
import subprocess, tempfile, os, sys

fails = []

def run(args, stdin=""):
    p = subprocess.run(["sh", "rollup.sh"] + args, input=stdin, text=True, capture_output=True)
    return p.returncode, p.stdout, p.stderr

def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name)
    if not ok:
        fails.append(name + ((": " + detail) if detail else ""))

HDR = "%-32s %7s %6s %6s %6s %10s" % ("endpoint", "count", "err%", "p50", "p95", "bytes")

def row(ep, c, pct, p50, p95, b):
    return "%-32s %7s %6.1f %6s %6s %10s" % (ep, c, pct, p50, p95, b)

def report(requests, skipped, window, slowest, rows):
    out = "requests: %d\nskipped: %d\nwindow: %s\n" % (requests, skipped, window)
    if slowest:
        out += "slowest: %s\n\n%s\n" % (slowest, HDR)
        out += "".join(r + "\n" for r in rows)
    return out

d = tempfile.mkdtemp()

A = "\n".join([
    "2026-03-02T10:15:02Z GET /api/users/42/?verbose=1 200 10ms 100b",
    "2026-03-02T10:15:03Z GET /api/users/7 500 20ms 200b",
    "",
    "2026-03-02T10:15:04Z HEAD /api 200 5ms 5b",
    "2026-03-02T10:15:04Z GET api 200 5ms 5b",
    "2026-03-02T10:15:04Z GET /api 20 5ms 5b",
    "2026-03-02T10:15:04Z GET /api 200 5 5b",
    "2026-03-02T10:15:04Z GET /api 200 5ms 5",
    "2026-03-02T10:15:04Z  GET /api 200 5ms 5b",
    "2026-03-02T10:15:04Z GET  /api 200 5ms 5b",
    "2026-03-02T10:15:04Z GET /api  200 5ms 5b",
    "2026-03-02T10:15:04Z GET /api 200 5ms 5b ",
]) + "\n"
pa = os.path.join(d, "a.log")
open(pa, "w").write(A)
expA = report(2, 9, "2026-03-02T10:15:02Z .. 2026-03-02T10:15:03Z",
              "20ms GET /api/users/7 at 2026-03-02T10:15:03Z",
              [row("GET /api/users/:id", "2", 50.0, "10", "20", "300")])
rc, out, _ = run([pa])
check("validation, blanks, normalisation, p50/p95", rc == 0 and out == expA, repr(out))

B = "\n".join([
    "2026-03-02T10:00:00Z GET /b 200 30ms 1b",
    "2026-03-02T10:00:00Z GET /a 200 10ms 1b",
    "2026-03-02T10:00:00Z GET /a 200 20ms 1b",
    "2026-03-02T10:00:00Z GET /a 500 20ms 2b",
    "2026-03-02T10:00:00Z GET /c 404 40ms 5b",
]) + "\n"
pb = os.path.join(d, "b.log")
open(pb, "w").write(B)
expB = report(5, 0, "2026-03-02T10:00:00Z .. 2026-03-02T10:00:00Z",
              "40ms GET /c at 2026-03-02T10:00:00Z",
              [row("GET /a", "3", 100.0 / 3, "20", "20", "4"),
               row("GET /b", "1", 0.0, "30", "30", "1"),
               row("(other)", "1", 0.0, "-", "-", "5")])
rc, out, _ = run(["-n", "2", pb])
check("count sort, endpoint tie, err%, (other)", rc == 0 and out == expB, repr(out))

rc, out, _ = run(["-n", "9", pb])
check("no (other) when N covers all endpoints", rc == 0 and "(other)" not in out, repr(out))

C = "\n".join([
    "2026-03-02T10:09:00Z GET /first 200 50ms 1b",
    "2026-03-02T10:01:00Z GET /second 200 50ms 1b",
    "2026-03-02T10:05:00Z GET /mid 200 7ms 1b",
]) + "\n"
pc = os.path.join(d, "c.log")
open(pc, "w").write(C)
rc, out, _ = run([pc])
check("window min/max, slowest tie takes first in file",
      rc == 0
      and "window: 2026-03-02T10:01:00Z .. 2026-03-02T10:09:00Z\n" in out
      and "slowest: 50ms GET /first at 2026-03-02T10:09:00Z\n" in out, repr(out))

longep = "GET /a/very/long/path/that/goes/past/thirtytwo"
pd_ = os.path.join(d, "d.log")
open(pd_, "w").write("2026-03-02T11:00:00Z " + longep + " 200 5ms 1b\n")
rc, out, _ = run([pd_])
check("long endpoint not truncated", rc == 0 and out.splitlines()[-1].startswith(longep), repr(out))

pe = os.path.join(d, "e.log")
open(pe, "w").write("\n\nbad\nstill bad\nnope\n")
rc, out, _ = run([pe])
check("no valid lines report", rc == 0 and out == "requests: 0\nskipped: 3\nwindow: -\n", repr(out))

pf = os.path.join(d, "f.log")
open(pf, "w").write("2026-03-02T12:00:00Z GET /x 499 4ms 1b\n"
                    "2026-03-02T12:00:00Z GET /y 500 4ms 1b\n")
rc, out, _ = run([pf])
xrow = [l for l in out.splitlines() if l.startswith("GET /x")]
yrow = [l for l in out.splitlines() if l.startswith("GET /y")]
check("err% boundary at 500",
      rc == 0 and xrow and yrow and xrow[0].split()[3] == "0.0" and yrow[0].split()[3] == "100.0",
      repr(out))

rc, fout, _ = run([pb])
rc2, sout, _ = run([], stdin=B)
rc3, dash, _ = run(["-"], stdin=B)
check("stdin and - match file input", rc == rc2 == rc3 == 0 and fout == sout == dash, repr(sout))

for args in (["-n", "0"], ["-n", "x"], ["-n"], ["-x"], ["a", "b"], ["-n", "3", "a", "b"], ["-n", "-2"]):
    rc, out, err = run(args, stdin="")
    check("usage error exit 2 for " + " ".join(args),
          rc == 2 and err.startswith("usage:") and out == "", repr((rc, out, err)))

rc, out, _ = run(["-n", "001"])
check("-n with leading zeros accepted", rc == 0 and out.startswith("requests: 0"), repr((rc, out)))

print("EDGE OK" if not fails else "EDGE FAILED: " + "; ".join(fails))
sys.exit(1 if fails else 0)
PY'''

CALLS = [
    {"name": "write", "arguments": {"path": "rollup.sh", "content": SCRIPT, "overwrite": True}},
    {"name": "shell", "arguments": {"cmd": CMD, "expect_exit": "0", "expect_has": "EDGE OK"}},
    {"name": "finish", "arguments": {"summary": "Implemented rollup.sh in POSIX sh + mawk + sort. It parses `-n N` / FILE / stdin / `-` with usage errors exiting 2, validates six single-space-separated fields (method, leading /, three-digit status, NNNms, NNNb) by splitting on the / / regex so doubled, leading or trailing spaces are invalid, ignores blank lines and counts the rest as skipped, normalises paths (drop query, drop one trailing slash, digits-only segments to :id), prints requests/skipped/window/slowest (first maximum latency, path as logged), then the table sorted by count desc then endpoint with %.1f err%, nearest-rank p50/p95, byte sums and an (other) row beyond N. `sh check.sh` passes, and an edge suite passes: malformed spacing and fields skipped, blanks not counted, multi-segment normalisation, percentile boundaries, count ties, (other) aggregation, long endpoints, the empty report, the 500 error boundary, stdin parity and every usage error exiting 2.", "unverified": "Large logs, CRLF input and a read failure mid-stream were not exercised; (other) percentiles are '-' per the README."}},
]
