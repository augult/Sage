THOUGHT = """I need to implement all of SPEC.md in conf/parser.py. Reading the spec, the starter parser, the visible tests and the example file gives me the exact rules and lets me see what the starter misses."""

CONTENT = """I'll read the specification, the current parser and the tests before rewriting it."""

CALLS = [
    {"name": "read", "arguments": {"filePath": "SPEC.md"}},
    {"name": "read", "arguments": {"filePath": "conf/parser.py"}},
    {"name": "read", "arguments": {"filePath": "tests/test_conf.py"}},
    {"name": "read", "arguments": {"filePath": "examples/app.conf"}},
]
