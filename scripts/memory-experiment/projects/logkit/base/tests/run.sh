#!/bin/sh
# Runs every case in tests/cases: NAME.cmd holds logkit's arguments, NAME.expected its stdout,
# NAME.in its stdin (tests/sample.in when there is none).
cd "$(dirname "$0")/.." || exit 1
pass=0
fail=0
for cmdfile in tests/cases/*.cmd; do
  name=${cmdfile%.cmd}
  input=$name.in
  [ -f "$input" ] || input=tests/sample.in
  actual=$(sh bin/logkit $(cat "$cmdfile") < "$input" 2>&1)
  if [ "$actual" = "$(cat "$name.expected")" ]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    echo "FAIL ${name##*/}: logkit $(cat "$cmdfile")"
    echo "  expected:"
    sed 's/^/    /' "$name.expected"
    echo "  actual:"
    printf '%s\n' "$actual" | sed 's/^/    /'
  fi
done
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
