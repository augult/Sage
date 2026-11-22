THOUGHT = "The core problem is simply that the reader separator is literal backslash-t; I’ll use POSIX printf octal to generate a real tab, then rerun the report and edge cases."
CONTENT = "The separator was the sole cause of the metadata parsing failure. I’m generating a real tab with printf and rerunning the checks."
CALLS = [
    {"name": "apply_patch", "arguments": {"patchText": r'''*** Begin Patch
*** Update File: rollup.sh
@@
-tab='\t'
+tab=$(printf '\\011')
*** End Patch'''}},
    {"name": "bash", "arguments": {"command": "set -e; sh -n rollup.sh; sh check.sh; printf '2026-01-01T00:00:00Z GET /x/12/?q=1 200 1ms 2b\\n2026-01-01T00:00:01Z GET /x/13 500 5ms 3b\\n' | sh rollup.sh -n 1 -; printf 'bad\\n' | sh rollup.sh -", "timeout": 30, "workdir": "."}},
]
