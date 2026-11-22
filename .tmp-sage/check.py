import json
import os
import subprocess
import sys
import tempfile

root = os.getcwd()
base_env = {"PATH": os.environ["PATH"], "PYTHONPATH": root, "HOME": os.environ.get("HOME", "/tmp")}


def run_cli(args, extra=None):
    env = dict(base_env)
    env.update(extra or {})
    return subprocess.run([sys.executable, "-m", "fetchkit"] + args, cwd=root, env=env, capture_output=True, text=True)


hits = subprocess.run(["rg", "-n", "retries", ".", "-g", "*.py", "-g", "*.md", "-g", "*.json", "-g", "*.sh",
                       "-g", "*.template", "-g", "!docs/attempts-migration.md"],
                      cwd=root, capture_output=True, text=True)
if hits.stdout.strip():
    raise SystemExit("old name survives:\n" + hits.stdout)

from fetchkit import DEFAULT_ATTEMPTS, Client, RetryError, backoff_delays, call_with_retry
from fetchkit.config import ConfigError, load_config
from fetchkit.jobs import PRIORITY_ATTEMPTS, Job, ManifestError, load_manifest
from fetchkit.transport import SimulatedTransport

assert DEFAULT_ATTEMPTS == 3
assert Client(sleep=lambda _: None).attempts == 3
assert Client(sleep=lambda _: None).budget_s == 30.0
assert backoff_delays(1) == []
assert backoff_delays(3, base=1.0) == [1.0, 2.0]

for bad in (0, -1):
    try:
        Client(attempts=bad)
    except ValueError:
        pass
    else:
        raise SystemExit("Client accepted attempts=" + repr(bad))
    try:
        call_with_retry(lambda: 1, attempts=bad)
    except ValueError:
        pass
    else:
        raise SystemExit("call_with_retry accepted attempts=" + repr(bad))

try:
    Client(retries=1)
except TypeError:
    pass
else:
    raise SystemExit("Client still takes retries")

transport = SimulatedTransport(fail_first=2, trace="")
try:
    Client(transport=transport, attempts=2, sleep=lambda _: None).fetch("u")
except RetryError as error:
    assert error.attempts == 2
    assert str(error) == "gave up after 2 attempts: simulated failure 2 for u", str(error)
else:
    raise SystemExit("no RetryError")

assert load_config(env={})["attempts"] == 3
assert load_config(env={"FETCHKIT_ATTEMPTS": "5"})["attempts"] == 5
try:
    load_config(env={"FETCHKIT_ATTEMPTS": "0"})
except ConfigError:
    pass
else:
    raise SystemExit("env accepted zero attempts")

old = tempfile.mkstemp(suffix=".ini")[1]
with open(old, "w", encoding="utf-8") as handle:
    handle.write("[fetch]" + chr(10) + "retries = 4" + chr(10))
try:
    load_config(old, env={})
except ConfigError as error:
    assert "retries" in str(error)
else:
    raise SystemExit("config still accepts retries")
with open(old, "w", encoding="utf-8") as handle:
    handle.write("[fetch]" + chr(10) + "attempts = 4" + chr(10))
assert load_config(old, env={})["attempts"] == 4
os.unlink(old)

assert PRIORITY_ATTEMPTS == {"low": 1, "normal": None, "high": 6}
assert Job("u", priority="high").attempts == 6
assert Job("u", attempts=2).attempts == 2
for bad in (0, -1, 1.5, "2"):
    try:
        Job("u", attempts=bad)
    except ManifestError:
        pass
    else:
        raise SystemExit("Job accepted attempts=" + repr(bad))

bad_manifest = tempfile.mkstemp(suffix=".json")[1]
with open(bad_manifest, "w", encoding="utf-8") as handle:
    json.dump({"jobs": [{"url": "u", "retries": 1}]}, handle)
try:
    load_manifest(bad_manifest)
except ManifestError as error:
    assert "retries" in str(error)
else:
    raise SystemExit("manifest still accepts retries")
os.unlink(bad_manifest)

result = run_cli(["--help"])
assert "--attempts" in result.stdout and "--retries" not in result.stdout
result = run_cli(["show-config"])
assert "attempts = 3" in result.stdout, result.stdout
result = run_cli(["show-config", "--attempts", "5"])
assert "attempts = 5" in result.stdout, result.stdout
result = run_cli(["show-config", "--retries", "5"])
assert result.returncode == 2, result.returncode
result = run_cli(["show-config", "--attempts", "0"])
assert result.returncode == 2 and "at least 1" in result.stderr, result.stderr
result = run_cli(["fetch", "--attempts", "2", "https://m.example/x"], {"FETCHKIT_FAIL_FIRST": "2"})
assert result.returncode == 1, result.stdout + result.stderr
assert "FAILED https://m.example/x after 2 attempts" in result.stdout, result.stdout
result = run_cli(["fetch", "https://m.example/x"], {"FETCHKIT_FAIL_FIRST": "3"})
assert result.returncode == 1 and "after 3 attempts" in result.stdout, result.stdout
result = run_cli(["fetch", "https://m.example/x"], {"FETCHKIT_FAIL_FIRST": "0"})
assert result.returncode == 0 and "ok https://m.example/x (1 tries)" in result.stdout, result.stdout
result = run_cli(["fetch", "--attempts", "1", "--manifest", "examples/manifest.json", "https://m.example/z"],
                 {"FETCHKIT_FAIL_FIRST": "0"})
assert result.returncode == 0, result.stdout + result.stderr

trace = os.path.join(tempfile.mkdtemp(), "tries.log")
script_env = dict(os.environ)
script_env.update({"FETCHKIT_FAIL_FIRST": "99", "FETCHKIT_TRACE": trace, "FETCHKIT_BACKOFF": "0"})
for key in list(script_env):
    if key.startswith("FETCHKIT_") and key not in ("FETCHKIT_FAIL_FIRST", "FETCHKIT_TRACE", "FETCHKIT_BACKOFF"):
        del script_env[key]
result = subprocess.run(["sh", "scripts/nightly.sh"], cwd=root, env=script_env, capture_output=True, text=True)
assert result.returncode == 1, result.stdout + result.stderr
counts = {}
for line in open(trace, encoding="utf-8"):
    url = line.rsplit(" ", 1)[0]
    counts[url] = counts.get(url, 0) + 1
expected = {
    "https://mirror.example/index.json": 6,
    "https://mirror.example/packages/core.tar": 5,
    "https://mirror.example/packages/extras.tar": 1,
    "https://mirror.example/packages/docs.tar": 2,
    "https://mirror.example/keys/release.asc": 1,
}
assert counts == expected, counts
os.unlink(trace)

result = subprocess.run(["sh", "scripts/smoke.sh"], cwd=root, env=script_env, capture_output=True, text=True)
assert result.returncode == 1, result.stdout + result.stderr
counts = {}
for line in open(trace, encoding="utf-8"):
    url = line.rsplit(" ", 1)[0]
    counts[url] = counts.get(url, 0) + 1
assert counts == {"https://mirror.example/index.json": 1}, counts
os.unlink(trace)

ok_env = dict(os.environ)
ok_env.update({"FETCHKIT_FAIL_FIRST": "0", "FETCHKIT_TRACE": trace})
for key in list(ok_env):
    if key.startswith("FETCHKIT_") and key not in ("FETCHKIT_FAIL_FIRST", "FETCHKIT_TRACE"):
        del ok_env[key]
result = subprocess.run([sys.executable, "scripts/backfill.py", "2019"], cwd=root, env=ok_env, capture_output=True, text=True)
assert result.returncode == 0, result.stdout + result.stderr
assert "fetched https://mirror.example/archive/2019.tar" in result.stdout, result.stdout
os.unlink(trace)
probe_env = dict(ok_env)
probe_env["FETCHKIT_FAIL_FIRST"] = "1"
result = subprocess.run([sys.executable, "scripts/backfill.py", "2019"], cwd=root, env=probe_env, capture_output=True, text=True)
assert result.returncode == 1, result.stdout + result.stderr
assert "index unreachable" in result.stdout, result.stdout
assert "gave up after 1 attempts" in result.stdout, result.stdout

print("ALL CHECKS OK")
