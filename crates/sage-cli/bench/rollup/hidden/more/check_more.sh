#!/bin/sh
# Held-out checks for rollup.sh: a larger log, the options, and the tools of the log hosts.
cd "$(dirname "$0")/.." || exit 1
here=more
status=0

# The log hosts have no scripting languages and no gawk.
shim=$(mktemp -d) || exit 1
trap 'rm -rf "$shim"' EXIT
for tool in python python3 perl node ruby gawk php lua; do
    printf '#!/bin/sh\necho "%s: not installed on the log hosts" >&2\nexit 127\n' "$tool" > "$shim/$tool"
    chmod +x "$shim/$tool"
done
PATH="$shim:$PATH"
export PATH

same() {
    # same NAME EXPECTED -- COMMAND...
    name=$1 expected=$2
    shift 3
    if "$@" 2> "$shim/stderr" | diff - "$expected" > "$shim/diff"; then
        :
    else
        echo "FAIL: $name"
        head -n 12 "$shim/diff"
        head -n 3 "$shim/stderr"
        status=1
    fi
}

exits() {
    # exits NAME STATUS -- COMMAND...
    name=$1 wanted=$2
    shift 3
    "$@" > /dev/null 2>&1 < /dev/null
    got=$?
    if [ "$got" -ne "$wanted" ]; then
        echo "FAIL: $name: exit status $got, expected $wanted"
        status=1
    fi
}

same "big log" $here/big.txt -- sh rollup.sh $here/big.log
same "big log, -n 5" $here/big_top5.txt -- sh rollup.sh -n 5 $here/big.log
same "big log, -n 100" $here/big_top100.txt -- sh rollup.sh -n 100 $here/big.log
same "no valid line" $here/junk.txt -- sh rollup.sh $here/junk.log
same "one request" $here/tiny.txt -- sh rollup.sh $here/tiny.log
same "sample" expected.txt -- sh rollup.sh access.log
same "sample, -n 3" expected_top3.txt -- sh rollup.sh -n 3 access.log

if sh rollup.sh < $here/big.log 2> /dev/null | diff - $here/big.txt > /dev/null; then :; else
    echo "FAIL: log on standard input, no FILE"
    status=1
fi
if sh rollup.sh -n 5 - < $here/big.log 2> /dev/null | diff - $here/big_top5.txt > /dev/null; then :; else
    echo "FAIL: log on standard input, FILE is -"
    status=1
fi

exits "unknown option" 2 -- sh rollup.sh -x $here/tiny.log
exits "-n without a number" 2 -- sh rollup.sh -n
exits "-n 0" 2 -- sh rollup.sh -n 0 $here/tiny.log
exits "-n abc" 2 -- sh rollup.sh -n abc $here/tiny.log
exits "two files" 2 -- sh rollup.sh $here/tiny.log $here/junk.log
exits "a good run" 0 -- sh rollup.sh -n 2 $here/tiny.log

[ "$status" -eq 0 ] && echo "check_more.sh: ok"
exit "$status"
