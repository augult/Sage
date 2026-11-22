THOUGHT = "Explore the repository: README, file layout, and any existing implementation."

CONTENT = "Let me look at the project structure and the README first."

CALLS = [
    {"name": "bash", "arguments": {"command": "cat README.md Cargo.toml; echo '--- src ---'; ls -la src; echo '--- files ---'; find src -type f | head -50"}},
]
