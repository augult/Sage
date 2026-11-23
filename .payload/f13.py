import contextlib
import io
import json
import os
import pathlib
import sys
import tempfile

sys.path.insert(0, ".")

from fetchkit import DEFAULT_ATTEMPTS, Client, RetryError, backoff_delays, call_with_retry
from fetchkit import cli
from fetchkit.config import DEFAULTS, ConfigError, load_config
from fetchkit.jobs import PRIORITY_ATTEMPTS, Job, ManifestError, load_manifest
from fetchkit.transport import SimulatedTransport


def raises(exc, fn, *args, **kwargs):
    try:
        fn(*args, **kwargs)
    except exc as error:
        return error
    raise AssertionError(f"{fn!r} did not raise {exc.__name__}")


def failing():
    raise OSError("boom")


assert DEFAULT_ATTEMPTS == 3
assert Client().attempts == 3
assert Client().budget_s == 30.0
assert Client(attempts=1, timeout=2.5).budget_s == 2.5
assert raises(ValueError, Client, attempts=0)
assert raises(ValueError, Client, attempts=-2)
assert raises(TypeError, Client, retries=1)
assert raises(ValueError, call_with_retry, failing, attempts=0)
assert raises(ValueError, call_with_retry, failing, attempts=-1)
assert backoff_delays(1) == []
assert backoff_delays(2, base=1.0) == [1.0]
assert backoff_delays(4, base=1.0) == [1.0, 2.0, 4.0]

error = raises(RetryError, call_with_retry, failing, attempts=2, sleep=lambda _: None)
assert error.attempts == 2, error.attempts
assert str(error) == "gave up after 2 attempts: boom", str(error)

client = Client(transport=SimulatedTransport(fail_first=9, trace=""), sleep=lambda _: None)
raises(RetryError, client.fetch, "http://x/")
assert client.transport.calls["http://x/"] == 3
client = Client(transport=SimulatedTransport(fail_first=9, trace=""), sleep=lambda _: None)
error = raises(RetryError, client.fetch, "http://x/", attempts=2)
assert error.attempts == 2 and client.transport.calls["http://x/"] == 2
assert raises(TypeError, client.fetch, "http://x/", retries=1)
single = Client(transport=SimulatedTransport(fail_first=0, trace=""), sleep=lambda _: None)
assert single.fetch("http://y/", attempts=1)
assert single.transport.calls["http://y/"] == 1
one = Client(transport=SimulatedTransport(fail_first=1, trace=""), sleep=lambda _: None)
assert raises(RetryError, one.fetch, "http://z/", attempts=1)

assert DEFAULTS["attempts"] == 3
assert load_config(None, env={"FETCHKIT_ATTEMPTS": "1"})["attempts"] == 1
assert raises(ConfigError, load_config, None, {"FETCHKIT_ATTEMPTS": "0"})
assert raises(ConfigError, load_config, None, {"FETCHKIT_ATTEMPTS": "x"})
template = load_config("config/fetchkit.ini.template")
assert template["attempts"] == 5, template

directory = tempfile.mkdtemp()
stale = os.path.join(directory, "stale.ini")
pathlib.Path(stale).write_text("[fetch]\nretries = 4\n", encoding="utf-8")
assert "unknown option" in str(raises(ConfigError, load_config, stale))

assert PRIORITY_ATTEMPTS == {"low": 1, "normal": None, "high": 6}
assert Job("http://a/").attempts is None
assert Job("http://a/", priority="low").attempts == 1
assert Job("http://a/", priority="high").attempts == 6
assert Job("http://a/", attempts=1).attempts == 1
assert raises(ManifestError, Job, "http://a/", attempts=0)
assert raises(ManifestError, Job, "http://a/", attempts=-1)
jobs = load_manifest("examples/manifest.json")
assert [job.attempts for job in jobs] == [6, None, 1, 2, 1], [job.attempts for job in jobs]
bad = os.path.join(directory, "bad.json")
pathlib.Path(bad).write_text(json.dumps({"jobs": [{"url": "http://a/", "retries": 1}]}), encoding="utf-8")
assert "unknown key" in str(raises(ManifestError, load_manifest, bad))

os.environ["FETCHKIT_FAIL_FIRST"] = "1"
out, err = io.StringIO(), io.StringIO()
with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
    code = cli.main(["fetch", "--attempts", "2", "http://a/"])
assert code == 0, (code, out.getvalue(), err.getvalue())
assert out.getvalue() == "ok http://a/ (2 tries)\n", out.getvalue()

os.environ["FETCHKIT_FAIL_FIRST"] = "5"
out = io.StringIO()
with contextlib.redirect_stdout(out):
    code = cli.main(["fetch", "--attempts", "2", "http://b/"])
assert code == 1, code
assert out.getvalue() == "FAILED http://b/ after 2 attempts\n", out.getvalue()

out = io.StringIO()
with contextlib.redirect_stdout(out):
    assert cli.main(["show-config", "--attempts", "4"]) == 0
assert "attempts = 4" in out.getvalue(), out.getvalue()

out, err = io.StringIO(), io.StringIO()
with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
    try:
        cli.main(["fetch", "--retries", "2", "http://c/"])
        raise AssertionError("--retries was accepted")
    except SystemExit as exit_error:
        assert exit_error.code == 2, exit_error.code
os.environ.pop("FETCHKIT_FAIL_FIRST", None)

root = pathlib.Path(".")
skip = {".git", "docs", "tests"}
offenders = []
for path in root.rglob("*"):
    if not path.is_file() or path.name == "_check.py":
        continue
    if skip & set(path.parts):
        continue
    try:
        text = path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, OSError):
        continue
    if "retries" in text:
        offenders.append(str(path))
assert not offenders, offenders

print("ALL CHECKS PASSED")
