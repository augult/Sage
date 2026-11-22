THOUGHT = "Starting by inspecting the repo: list files and read the exercise README."

CONTENT = "Let me look at the repository and the exercise description."

CALLS = [
    {"tool": "bash", "input": {"command": "ls -la && echo '---' && cat README.md"}},
]
CONTENT = ""
CALLS = [
    # {"name": "...", "arguments": {...}},
]
