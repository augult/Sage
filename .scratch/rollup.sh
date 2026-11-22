#!/bin/sh
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
	nf = split(line, f, " ")
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
		printf "%s %s\n", first, last > sumf
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

LC_ALL=C sort -t "$tab" -k1,1nr -k2,2 "$tmp/rows" > "$tmp/rows.sorted"

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
