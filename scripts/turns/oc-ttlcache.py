THOUGHT = "The full cache implementation is restored; run the provided test suite and inspect the final diff for contract issues."
CONTENT = ""
CALLS = [
    {"name": "bash", "arguments": {"command": "node --test", "timeout": 120000, "workdir": "."}},
    {"name": "bash", "arguments": {"command": "git diff --check && git diff -- src/cache.js", "timeout": 120000, "workdir": "."}}
]
