THOUGHT = r'''Now let me look at the crate scaffolding and any existing tests.'''

CONTENT = ""

CALLS = [
    {"name": "read", "arguments": {"filePath": "Cargo.toml"}},
    {"name": "bash", "arguments": {"command": "ls -la src && find . -path ./.git -prune -o -type f -print"}},
    {"name": "read", "arguments": {"filePath": "src/lib.rs"}},
    {"name": "bash", "arguments": {"command": "cat tests/alloc-attack.rs 2>/dev/null; echo '---'; cat tests/forth.rs 2>/dev/null | head -100"}},
]
