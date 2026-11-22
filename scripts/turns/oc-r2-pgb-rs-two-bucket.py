THOUGHT = "Inspect the Rust crate API and dependencies before implementing the transfer simulation."
CONTENT = ""
CALLS = [
    {"name": "read", "arguments": {"filePath": "src/lib.rs", "offset": 0, "limit": 16000}},
    {"name": "read", "arguments": {"filePath": "Cargo.toml", "offset": 0, "limit": 4000}}
]
