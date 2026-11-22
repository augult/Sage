THOUGHT = """The task is to implement SPEC.md fully in conf/parser.py. First I need to read the spec, the current parser, the visible tests and the example file so the implementation matches every rule and the line-number reporting."""
CONTENT = """I'll start by reading the specification, the current parser, the tests and the example file."""
CALLS = [
    {"name": "read", "arguments": {"filePath": "SPEC.md"}},
    {"name": "read", "arguments": {"filePath": "conf/parser.py"}},
    {"name": "read", "arguments": {"filePath": "tests/test_conf.py"}},
    {"name": "read", "arguments": {"filePath": "examples/app.conf"}},
    {"name": "bash", "arguments": {"command": "python3 -m unittest discover -s tests"}},
]
