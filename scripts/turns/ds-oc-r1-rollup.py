THOUGHT = r'''The sample report matches. The spec says the script must hold for any log and the whole
command line, so I want to exercise stdin, "-", empty input, malformed command lines, an empty
log, an all-skipped log, count ties (byte-order tie-break), an endpoint longer than 32 columns
and N larger than the number of endpoints.'''

CONTENT = r'''`check.sh` passes. Let me exercise the rest of the contract - stdin and `-`, empty and all-invalid logs, bad arguments, ties, a long endpoint and a large `-n`.'''

CALLS = [
    {"name": "bash", "arguments": {"command": r'''set +e
echo '== stdin =='; sh rollup.sh < access.log | head -2
echo '== dash file =='; sh rollup.sh - < access.log | head -2
echo '== empty input =='; printf '' | sh rollup.sh; echo "exit=$?"
echo '== all skipped =='; printf 'nope\n\n2026-03-02T10:15:02Z GET /x 20 5ms 1b\n' | sh rollup.sh; echo "exit=$?"
echo '== bad args =='
sh rollup.sh -n 0 access.log >/dev/null 2>&1; echo "n0=$?"
sh rollup.sh -n x access.log >/dev/null 2>&1; echo "nx=$?"
sh rollup.sh access.log extra >/dev/null 2>&1; echo "extra=$?"
sh rollup.sh -x >/dev/null 2>&1; echo "unknown=$?"
echo '== ties and long endpoint =='
printf '2026-01-01T00:00:00Z GET /b 200 1ms 5b\n2026-01-01T00:00:00Z GET /a 200 2ms 5b\n2026-01-01T00:00:00Z GET /a/very/long/segment/that/exceeds/thirtytwo/chars 500 3ms 7b\n' | sh rollup.sh
echo '== n larger than endpoints =='; sh rollup.sh -n 100 access.log | tail -2
echo '== stdin with -n 3 =='; sh rollup.sh -n 3 < access.log | diff - expected_top3.txt && echo same
echo '== temp cleanup =='; ls -d /tmp/rollup.* 2>/dev/null | wc -l'''}},
]
