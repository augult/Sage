THOUGHT = "The previous rerun exposed a shell syntax error in the generated script and flawed metadata field parsing. I’ll inspect the exact file before repairing both."
CONTENT = "The latest check exposed a shell quoting problem and metadata parsing bug; I’m inspecting the script to pinpoint them."
CALLS = [
    {"name": "read", "arguments": {"filePath": "rollup.sh", "offset": 0, "limit": 6000}},
]
