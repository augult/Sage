THOUGHT = "The failures confirm all tests expect the new API. I’ll read the implementation and shipped references, excluding the transport and test files, before planning a consistent migration."
CONTENT = ""
CALLS = [
    {"name": "read", "arguments": {"filePath": "fetchkit/retry.py", "offset": 0, "limit": 300}},
    {"name": "read", "arguments": {"filePath": "fetchkit/client.py", "offset": 0, "limit": 300}},
    {"name": "read", "arguments": {"filePath": "fetchkit/config.py", "offset": 0, "limit": 300}},
    {"name": "read", "arguments": {"filePath": "fetchkit/cli.py", "offset": 0, "limit": 300}},
    {"name": "read", "arguments": {"filePath": "fetchkit/jobs.py", "offset": 0, "limit": 300}},
    {"name": "read", "arguments": {"filePath": "fetchkit/__init__.py", "offset": 0, "limit": 100}},
    {"name": "read", "arguments": {"filePath": "README.md", "offset": 0, "limit": 300}},
    {"name": "read", "arguments": {"filePath": "config/fetchkit.ini.template", "offset": 0, "limit": 100}},
    {"name": "read", "arguments": {"filePath": "examples/manifest.json", "offset": 0, "limit": 100}},
    {"name": "read", "arguments": {"filePath": "scripts/backfill.py", "offset": 0, "limit": 250}},
    {"name": "read", "arguments": {"filePath": "scripts/smoke.sh", "offset": 0, "limit": 120}},
    {"name": "read", "arguments": {"filePath": "scripts/nightly.sh", "offset": 0, "limit": 120}},
    {"name": "grep", "arguments": {"pattern": "retries|RETRIES|Retry", "path": ".", "include": "*"}},
]
