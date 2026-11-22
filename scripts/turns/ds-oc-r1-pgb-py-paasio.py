THOUGHT = r'''Read the README and the existing files first.'''

CONTENT = r'''Let me inspect the exercise and the starting files.'''

CALLS = [
    {"name": "bash", "arguments": {"command": "pwd && find . -path ./.git -prune -o -type f -print && ls -la"}},
    {"name": "read", "arguments": {"filePath": "README.md"}},
]
