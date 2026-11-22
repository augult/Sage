THOUGHT = r'''The contract checks pass, but the shell wrapper cut the chained `npm test` short. I'll run the suite on its own and confirm no temporary file is left behind.'''

CONTENT = r'''The contract checks pass. Now the suite itself, on its own so the output is clean.'''

CALLS = [
    {"name": "bash", "arguments": {"command": "npm test && ls verify.tmp.mjs 2>/dev/null; echo done", "timeout": 600, "workdir": "."}},
]
