THOUGHT = "The provided fixtures and current CLI checks pass. I’ll probe all-invalid and representative normalization/ordering behavior, then finish the single benchmark instance."
CONTENT = "The supplied fixture checks now pass. I’m running a small final probe for the empty report and normalized routes."
CALLS = [
    {"name": "bash", "arguments": {"command": "set -e; sh -n rollup.sh; sh check.sh; out=$(printf 'bad line\\n\\n' | sh rollup.sh -); printf '%s\\n' \"$out\"; printf '2026-01-01T00:00:00Z GET /x/12/?q=1 200 1ms 2b\\n2026-01-01T00:00:01Z GET /x/13 500 5ms 3b\\n' | sh rollup.sh -n 1 -", "timeout": 30, "workdir": "."}},
]
