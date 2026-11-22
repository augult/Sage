THOUGHT = "I will inspect the Rust source and existing tests to determine the expected API."
CONTENT = ""
CALLS = [
    {"name": "glob", "arguments": {"pattern": "**/*", "path": "."}},
    {"name": "read", "arguments": {"filePath": "Cargo.toml", "offset": 0, "limit": 100}},
]
