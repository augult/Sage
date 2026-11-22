THOUGHT = r'''I need to read the task README and see the repo structure before writing code.'''

CONTENT = ""

CALLS = [
    {"name": "bash", "arguments": {"command": "pwd && ls -la"}},
    {"name": "read", "arguments": {"filePath": "README.md"}},
]
