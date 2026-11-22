THOUGHT = r'''Let me read the README and scaffolding first.'''

CONTENT = ""

CALLS = [
    {"name": "bash", "arguments": {"command": "pwd && ls -la && find . -path ./.git -prune -o -type f -print"}},
    {"name": "read", "arguments": {"filePath": "README.md"}},
    {"name": "read", "arguments": {"filePath": "src/lib.rs"}},
    {"name": "read", "arguments": {"filePath": "Cargo.toml"}},
]
