THOUGHT = r'''The task is to write rollup.sh from the README spec, targeting a minimal POSIX environment:
sh, mawk, sort/sed/tr/cut/head only. I need the exact expected output to get the column
widths and percentile math right, so first I will read the README, check.sh and look at the
log and current placeholder.'''

CONTENT = r'''I'll start by reading the spec and the checker, then look at the log and the current placeholder script.'''

CALLS = [
    {"name": "read", "arguments": {"filePath": "README.md"}},
    {"name": "read", "arguments": {"filePath": "check.sh"}},
    {"name": "bash", "arguments": {"command": "pwd; ls -la; echo '--- rollup.sh ---'; cat rollup.sh; echo '--- log ---'; cat access.log"}},
]
