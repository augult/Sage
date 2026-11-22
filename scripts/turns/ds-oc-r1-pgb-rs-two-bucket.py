THOUGHT = "Inspect the README, manifest and skeleton."
CONTENT = "Reading the exercise description and current source."
CALLS = [
    {"name": "bash", "arguments": {"command": "ls -la && echo '--- README ---' && cat README.md && echo '--- CARGO ---' && cat Cargo.toml && echo '--- LIB ---' && cat src/lib.rs && echo '--- TREE ---' && find . -path ./target -prune -o -path ./.git -prune -o -type f -print"}},
]
