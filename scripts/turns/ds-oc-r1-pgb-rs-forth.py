THOUGHT = "Need the existing public API in src/lib.rs to match the grader's tests."
CONTENT = "Reading the stub API."
CALLS = [
    {"name": "bash", "arguments": {"command": "cat src/lib.rs; echo ---; cat Cargo.toml; echo ---; git log --oneline -5; git status --short"}},
]
