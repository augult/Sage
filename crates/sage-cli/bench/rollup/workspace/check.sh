#!/bin/sh
# Compares the report for access.log with the expected ones.
cd "$(dirname "$0")" || exit 1
status=0
sh rollup.sh access.log | diff - expected.txt || status=1
sh rollup.sh -n 3 access.log | diff - expected_top3.txt || status=1
[ "$status" -eq 0 ] && echo "check.sh: ok"
exit "$status"
