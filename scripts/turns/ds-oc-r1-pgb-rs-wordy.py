THOUGHT = "Read the README and the stub source to learn the required API."

CONTENT = "Let me look at the README and source."

CALLS = [
    {"name": "bash", "arguments": {"command": "ls; echo '--- README ---'; cat README.md; echo '--- Cargo.toml ---'; cat Cargo.toml; echo '--- src ---'; find src -type f -exec sh -c 'echo \"### $1\"; cat -n \"$1\"' _ {} \\;"}},
]
