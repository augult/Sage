THOUGHT = "Run the test suite."

CONTENT = "Running cargo test."

CALLS = [
    {"name": "bash", "arguments": {"command": "ls tests; cargo test 2>&1 | tail -30"}},
]
