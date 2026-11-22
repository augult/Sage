THOUGHT = "Explore the repository: README, file layout, and any existing implementation."

CONTENT = "Let me look at the project structure and the README first."

CALLS = [
    ("read", {"filePath": "README.md"}),
    ("glob", {"pattern": "**/*"}),
]
