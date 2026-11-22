THOUGHT = "Start by listing the repo and reading README.md to see what the exercise requires."

CONTENT = "Let me inspect the repository and read the task description."

CALLS = [
    {"name": "bash", "arguments": {"command": "ls -la"}},
    {"name": "read", "arguments": {"filePath": "README.md"}},
]
