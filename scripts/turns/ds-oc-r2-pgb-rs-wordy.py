THOUGHT = """Word problem parsing needs the exact README grammar and the stub signature. Reading both up front."""
CONTENT = """Let me read the exercise and the crate skeleton."""
CALLS = [
    {"name": "read", "arguments": {"filePath": "README.md"}},
    {"name": "bash", "arguments": {"command": "cat src/lib.rs; echo '---'; cat Cargo.toml; echo '---'; find . -type f -not -path './.git/*' | head -30"}},
]
