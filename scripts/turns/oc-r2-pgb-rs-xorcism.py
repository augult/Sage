THOUGHT = "Read the exercise requirements and starter API."
CONTENT = ""
CALLS = [
    {"name": "read", "arguments": {"filePath": "README.md", "offset": 0, "limit": 10000}},
    {"name": "read", "arguments": {"filePath": "src/lib.rs", "offset": 0, "limit": 10000}},
    {"name": "read", "arguments": {"filePath": "Cargo.toml", "offset": 0, "limit": 4000}}
]
