THOUGHT = r'''Start by reading the contract and the existing implementation plus tests.'''
CONTENT = "Let me read the contract, current implementation and tests."
CALLS = [
    {"name": "bash", "arguments": {"command": "cat README.md && echo '=== cache.js ===' && cat src/cache.js && echo '=== clock.js ===' && cat src/clock.js && echo '=== index.js ===' && cat src/index.js && echo '=== test ===' && cat test/cache.test.js"}},
]
