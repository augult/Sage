THOUGHT = """I need the README spec and the stub API before implementing the XOR cipher."""
CONTENT = """Reading the exercise description and the crate skeleton."""
CALLS = [
    {"name": "read", "arguments": {"filePath": "README.md"}},
    {"name": "bash", "arguments": {"command": "cat src/lib.rs; echo '---'; cat Cargo.toml; echo '---'; find . -type f -not -path './.git/*' | head -30"}},
]
