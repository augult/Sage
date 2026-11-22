#!/bin/sh
# rollup.sh: per-endpoint summary of a gateway access log. See README.md.

usage () {
	echo "usage: rollup.sh [-n N] [FILE]" >&2
	exit 2
}

n=10
file=

while [ $# -gt 0 ]; do
	case $1 in
	-n)
		[ $# -ge 2 ] || usage
		case $2 in
		''|*[!0-9]*) usage ;;
		esac
		n=$2
		shift 2
		;;
	-)
		[ -z "$file" ] || usage
		file=-
		shift
		;;
	-*)
		usage
		;;
	*)
		[ -z "$file" ] || usage
		file=$1
		shift
		;;
	esac
done

# N is a positive integer: refuse a value that is zero once leading zeros go.
while :; do
	case $n in
	0*) n=${n#0} ;;
	*) break ;;
	esac
done
[ -n "$n" ] || usage

if [ -n "$file" ] && [ "$file" != - ]; then
	[ -r "$file" ] || { echo "rollup.sh: cannot read $file" >&2; exit 2; }
fi

tab=`printf '\t'`
tmp=`mktemp -d "${TMPDIR:-/tmp}/rollup.XXXXXX"` || exit 1
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

if [ -z "$file" ] || [ "$file" = - ]; then
	cat
else
	cat "$file"
fi | awk -v rec="$tmp/rec" -v lat="$tmp/lat" -v sum="$tmp/sum" -v slow="$tmp/slow" '
{
	if ($0 == "") next
	if ($0 !~ /^[^ ]+ [^ ]+ [^ ]+ [^ ]+ [^ ]+ [^ ]+$/) { skipped++; next }
	ts = $1; method = $2; path = $3; status = $4; latv = $5; size = $6
	if (ts !~ /^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z$/) { skipped++; next }
	if (method !~ /^(GET|POST|PUT|PATCH|DELETE)$/) { skipped++; next }
	if (path !~ /^\//) { skipped++; next }
	if (status !~ /^[0-9][0-9][0-9]$/) { skipped++; next }
	if (latv !~ /^[0-9]+ms$/) { skipped++; next }
	if (size !~ /^[0-9]+b$/) { skipped++; next }

	valid++
	if (valid == 1 || ts < first) first = ts
	if (valid == 1 || ts > last) last = ts

	ms = latv; sub(/ms$/, "", ms); ms += 0
	by = size; sub(/b$/, "", by); by += 0

	if (valid == 1 || ms > best) { best = ms; bts = ts; bmeth = method; bpath = path }

	p = path
	sub(/\?.*$/, "", p)
	if (p != "/") sub(/\/$/, "", p)
	parts = split(p, seg, "/")
	np = ""
	for (i = 1; i <= parts; i++) {
		s = seg[i]
		if (s ~ /^[0-9]+$/) s = ":id"
		np = np (i > 1 ? "/" : "") s
	}
	ep = method " " np

	print ep "\t" ms "\t" status+0 "\t" by >> rec
	print ep "\t" ms >> lat
}
END {
	print valid+0 "\t" skipped+0 "\t" (valid ? first : "-") "\t" (valid ? last : "-") >> sum
	if (valid) print best "\t" bmeth "\t" bpath "\t" bts >> slow
}'

LC_ALL=C sort -t "$tab" -k1,1 -k2,2n "$tmp/lat" | awk -F'\t' '
BEGIN { OFS = "\t" }
{ c[$1]++; v[$1, c[$1]] = $2 }
END {
	for (e in c) {
		k = c[e]
		r50 = int((50 * k + 99) / 100)
		r95 = int((95 * k + 99) / 100)
		print e, v[e, r50], v[e, r95]
	}
}' > "$tmp/pct"

awk -F'\t' '
{ c[$1]++; if ($3 >= 500) err[$1]++; by[$1] += $4 }
END { for (e in c) print c[e] "\t" e "\t" err[e]+0 "\t" by[e]+0 }' "$tmp/rec" |
LC_ALL=C sort -t "$tab" -k1,1nr -k2,2 > "$tmp/rows"

IFS=$tab read -r req skp win_first win_last < "$tmp/sum"

printf 'requests: %s\n' "$req"
printf 'skipped: %s\n' "$skp"
if [ "$req" -eq 0 ]; then
	printf 'window: -\n'
	exit 0
fi
printf 'window: %s .. %s\n' "$win_first" "$win_last"
IFS=$tab read -r slow_ms slow_meth slow_path slow_ts < "$tmp/slow"
printf 'slowest: %sms %s %s at %s\n' "$slow_ms" "$slow_meth" "$slow_path" "$slow_ts"
printf '\n'
printf '%-32s %7s %6s %6s %6s %10s\n' endpoint count 'err%' p50 p95 bytes

awk -F'\t' -v n="$n" '
NR == FNR { p50[$1] = $2; p95[$1] = $3; next }
{
	rows++
	if (rows <= n + 0) {
		printf "%-32s %7d %6.1f %6d %6d %10d\n", $2, $1, ($1 ? $3 * 100.0 / $1 : 0), p50[$2], p95[$2], $4
	} else {
		oc += $1; oe += $3; ob += $4
	}
}
END {
	if (oc > 0) printf "%-32s %7d %6.1f %6s %6s %10d\n", "(other)", oc, oe * 100.0 / oc, "-", "-", ob
}' "$tmp/pct" "$tmp/rows"
