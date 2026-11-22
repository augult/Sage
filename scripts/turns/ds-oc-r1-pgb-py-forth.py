THOUGHT = r'''Read README and stub.'''

CONTENT = r'''Let me inspect the project.'''

CALLS = [
    {"name": "bash", "arguments": {"command": "pwd && ls -la && echo '--- README ---' && cat README.md && echo '--- STUB ---' && cat *.py"}},
]
