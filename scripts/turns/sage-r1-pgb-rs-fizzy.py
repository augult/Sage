THOUGHT = "Implement owned matcher predicates and substitution strings, then consume iterators with only the required Display and predicate bounds. The standard rules require remainder/equality and integer construction."
CONTENT = ""
CALLS = [
    {"name": "shell", "arguments": {"cmd": "cargo test && cargo clippy --all-targets -- -D warnings && git diff --check && git status --short", "expect_exit": "0", "timeout": 120}},
    {"name": "finish", "arguments": {"summary": "Implemented customizable generic FizzBuzz: matcher predicates and substitution words compose in registration order, unmatched displayable values pass through, any iterator can be consumed, and fizz_buzz supplies the standard rules. Unit/doc tests, warning-denied Clippy, focused executable behavior assertions, and diff checks passed.", "unverified": "The provided starter has no unit tests and grader tests were not available; README requirements are high-level only."}}
]
