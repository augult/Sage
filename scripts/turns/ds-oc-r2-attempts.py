THOUGHT = r'''Start by reading the agreed migration note, the tests that define the new API, and every
shipped file that mentions retries, so the change list is complete before I touch anything.'''

CONTENT = r'''I'll read the migration note, the tests and the current library, CLI, settings, manifest, scripts and README before migrating.'''

CALLS = [
    {"name": "todowrite", "arguments": {"todos": [
        {"content": "Read migration note, tests and current sources", "status": "in_progress", "priority": "high"},
        {"content": "Migrate library (retry, client, package exports)", "status": "pending", "priority": "high"},
        {"content": "Migrate settings, CLI and manifests", "status": "pending", "priority": "high"},
        {"content": "Migrate scripts, config template and README", "status": "pending", "priority": "high"},
        {"content": "Run tests and scan for leftover old names", "status": "pending", "priority": "high"},
    ]}},
    {"name": "read", "arguments": {"filePath": "docs/attempts-migration.md"}},
    {"name": "read", "arguments": {"filePath": "tests/test_retry.py"}},
    {"name": "read", "arguments": {"filePath": "tests/test_client.py"}},
    {"name": "read", "arguments": {"filePath": "fetchkit/retry.py"}},
    {"name": "read", "arguments": {"filePath": "fetchkit/client.py"}},
    {"name": "read", "arguments": {"filePath": "fetchkit/config.py"}},
    {"name": "read", "arguments": {"filePath": "fetchkit/cli.py"}},
    {"name": "read", "arguments": {"filePath": "fetchkit/jobs.py"}},
    {"name": "read", "arguments": {"filePath": "examples/manifest.json"}},
    {"name": "bash", "arguments": {"command": "for f in scripts/backfill.py scripts/nightly.sh scripts/smoke.sh config/fetchkit.ini.template fetchkit/__init__.py README.md; do echo \"=== $f\"; cat \"$f\"; done"}},
]
