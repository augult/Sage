import collections
import os
import re
import subprocess
import sys
import tempfile
import unittest

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
sys.path.insert(0, ROOT)

import fetchkit
from fetchkit import Client, RetryError, backoff_delays, call_with_retry
from fetchkit import jobs as jobs_module
from fetchkit.config import ConfigError, load_config
from fetchkit.jobs import ManifestError, load_manifest, run_jobs
from fetchkit.transport import SimulatedTransport

URL = "https://mirror.example/packages/core.tar"
BASE = "https://mirror.example/"


def client(fail_first, **settings):
    return Client(transport=SimulatedTransport(fail_first=fail_first, trace=""), sleep=lambda _: None, **settings)


def run(args, fail_first="0", extra_env=None):
    """Runs a command in the repository; returns (exit status, output, tries per URL)."""
    with tempfile.TemporaryDirectory() as tmp:
        trace = os.path.join(tmp, "trace")
        env = {k: v for k, v in os.environ.items() if not k.startswith("FETCHKIT_")}
        env.update({"FETCHKIT_FAIL_FIRST": fail_first, "FETCHKIT_TRACE": trace})
        env.update(extra_env or {})
        done = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True, timeout=60)
        tries = collections.Counter()
        if os.path.exists(trace):
            with open(trace, encoding="utf-8") as lines:
                for line in lines:
                    tries[line.split()[0].replace(BASE, "")] += 1
        return done.returncode, done.stdout + done.stderr, dict(tries)


def cli(*args, **kwargs):
    return run([sys.executable, "-m", "fetchkit", *args], **kwargs)


def write(tmp, name, text):
    path = os.path.join(tmp, name)
    with open(path, "w", encoding="utf-8") as out:
        out.write(text)
    return path


class LibraryTest(unittest.TestCase):
    def test_old_names_are_gone(self):
        with self.assertRaises(TypeError):
            Client(retries=1)
        with self.assertRaises(TypeError):
            client(0).fetch(URL, retries=0)
        with self.assertRaises(TypeError):
            call_with_retry(lambda: 1, retries=1)
        self.assertFalse(hasattr(fetchkit, "DEFAULT_RETRIES"))
        self.assertEqual(fetchkit.DEFAULT_ATTEMPTS, 3)

    def test_at_least_one_attempt(self):
        for bad in (0, -1):
            with self.assertRaises(ValueError):
                Client(attempts=bad)
            with self.assertRaises(ValueError):
                call_with_retry(lambda: 1, attempts=bad)
        self.assertEqual(call_with_retry(lambda: 7, attempts=1), 7)

    def test_default_is_three_tries(self):
        made = client(99)
        with self.assertRaises(RetryError):
            made.fetch(URL)
        self.assertEqual(made.transport.calls[URL], 3)

    def test_override_per_fetch(self):
        made = client(99, attempts=5)
        with self.assertRaises(RetryError):
            made.fetch(URL, attempts=1)
        self.assertEqual(made.transport.calls[URL], 1)

    def test_budget_is_unchanged(self):
        self.assertEqual(client(0, attempts=4, timeout=5.0).budget_s, 20.0)
        self.assertEqual(client(0, timeout=10.0).budget_s, 30.0)
        self.assertEqual(client(0, attempts=1, timeout=2.0).budget_s, 2.0)

    def test_retry_error(self):
        made = client(99, attempts=4)
        with self.assertRaises(RetryError) as caught:
            made.fetch(URL)
        self.assertEqual(caught.exception.attempts, 4)
        self.assertFalse(hasattr(caught.exception, "retries"))
        self.assertTrue(str(caught.exception).startswith("gave up after 4 attempts: "), str(caught.exception))

    def test_pauses(self):
        self.assertEqual(backoff_delays(1), [])
        self.assertEqual(backoff_delays(3, base=0.5), [0.5, 1.0])
        slept = []

        def fail():
            raise OSError("no")

        with self.assertRaises(RetryError):
            call_with_retry(fail, attempts=4, base=1.0, sleep=slept.append)
        self.assertEqual(slept, [1.0, 2.0, 4.0])


class ConfigTest(unittest.TestCase):
    def test_default(self):
        config = load_config(None, env={})
        self.assertEqual(config["attempts"], 3)
        self.assertNotIn("retries", config)

    def test_file_and_environment(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write(tmp, "a.ini", "[fetch]\nattempts = 5\n")
            self.assertEqual(load_config(path, env={})["attempts"], 5)
            self.assertEqual(load_config(path, env={"FETCHKIT_ATTEMPTS": "4"})["attempts"], 4)

    def test_rejects(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(ConfigError):
                load_config(write(tmp, "old.ini", "[fetch]\nretries = 2\n"), env={})
            with self.assertRaises(ConfigError):
                load_config(write(tmp, "zero.ini", "[fetch]\nattempts = 0\n"), env={})
            with self.assertRaises(ConfigError):
                load_config(None, env={"FETCHKIT_ATTEMPTS": "0"})

    def test_template_keeps_its_patience(self):
        config = load_config(os.path.join(ROOT, "config", "fetchkit.ini.template"), env={})
        self.assertEqual(config["attempts"], 5)
        self.assertEqual(config["timeout"], 20.0)


class CommandLineTest(unittest.TestCase):
    def test_show_config(self):
        status, out, _ = cli("show-config")
        self.assertEqual(status, 0, out)
        self.assertIn("attempts = 3", out.splitlines())
        self.assertFalse([line for line in out.splitlines() if line.startswith("retries")], out)
        self.assertIn("attempts = 6", cli("show-config", "--attempts", "6")[1].splitlines())
        self.assertIn("attempts = 4", cli("show-config", extra_env={"FETCHKIT_ATTEMPTS": "4"})[1].splitlines())

    def test_rejects(self):
        self.assertEqual(cli("show-config", "--retries", "2")[0], 2)
        self.assertEqual(cli("show-config", "--attempts", "0")[0], 2)

    def test_help(self):
        status, out, _ = cli("fetch", "--help")
        self.assertEqual(status, 0, out)
        self.assertNotIn("--retries", out)
        self.assertIn("--attempts", out)
        described = re.split(r"\n\s+-", out[out.rindex("--attempts") + len("--attempts"):], maxsplit=1)[0]
        self.assertTrue(re.search(r"\b3\b", described), described)
        self.assertFalse(re.search(r"\b2\b", described), described)

    def test_tries(self):
        status, out, tries = cli("fetch", "--attempts", "3", URL, fail_first="2")
        self.assertEqual((status, tries), (0, {"packages/core.tar": 3}), out)
        status, out, tries = cli("fetch", "--attempts", "2", URL, fail_first="2")
        self.assertEqual((status, tries), (1, {"packages/core.tar": 2}), out)
        self.assertIn(f"FAILED {URL} after 2 attempts", out)
        status, out, tries = cli("fetch", URL, fail_first="99")
        self.assertEqual((status, tries), (1, {"packages/core.tar": 3}), out)
        self.assertIn(f"FAILED {URL} after 3 attempts", out)


class ManifestTest(unittest.TestCase):
    def test_priorities(self):
        self.assertEqual(jobs_module.PRIORITY_ATTEMPTS, {"low": 1, "normal": None, "high": 6})
        self.assertFalse(hasattr(jobs_module, "PRIORITY_RETRIES"))

    def test_example_manifest_keeps_its_behaviour(self):
        jobs = load_manifest(os.path.join(ROOT, "examples", "manifest.json"))
        results = run_jobs(jobs, client(99))
        self.assertEqual([tries for _, tries, _ in results], [6, 3, 1, 2, 1])
        results = run_jobs(jobs, client(99, attempts=4))
        self.assertEqual([tries for _, tries, _ in results], [6, 4, 1, 2, 1])

    def test_rejects(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(ManifestError):
                load_manifest(write(tmp, "old.json", '{"jobs": [{"url": "u", "retries": 1}]}'))
            with self.assertRaises(ManifestError):
                load_manifest(write(tmp, "zero.json", '{"jobs": [{"url": "u", "attempts": 0}]}'))
            jobs = load_manifest(write(tmp, "new.json", '{"jobs": [{"url": "u", "attempts": 2, "priority": "high"}]}'))
            self.assertEqual(run_jobs(jobs, client(99))[0][1], 2)


class ScriptTest(unittest.TestCase):
    def test_nightly(self):
        status, out, tries = run(["sh", "scripts/nightly.sh"], fail_first="99")
        expected = {"index.json": 6, "packages/core.tar": 5, "packages/extras.tar": 1, "packages/docs.tar": 2, "keys/release.asc": 1}
        self.assertEqual((status, tries), (1, expected), out)
        status, out, _ = run(["sh", "scripts/nightly.sh"], fail_first="core:4")
        self.assertEqual(status, 0, out)

    def test_smoke(self):
        status, out, tries = run(["sh", "scripts/smoke.sh"], fail_first="1")
        self.assertEqual((status, tries), (1, {"index.json": 1}), out)
        status, out, tries = run(["sh", "scripts/smoke.sh"])
        self.assertEqual((status, tries), (0, {"index.json": 1}), out)

    def test_backfill(self):
        script = [sys.executable, "scripts/backfill.py", "2019"]
        status, out, tries = run(script, fail_first="index:1")
        self.assertEqual((status, tries), (1, {"index.json": 1}), out)
        status, out, tries = run(script, fail_first="archive:7")
        self.assertEqual((status, tries), (0, {"index.json": 1, "archive/2019.tar": 8}), out)
        status, out, tries = run(script, fail_first="archive:8")
        self.assertEqual((status, tries), (1, {"index.json": 1, "archive/2019.tar": 8}), out)


OLD = [
    r"--retries",
    r"RETRIES",
    r"\bretries\s*=(?!=)",
    r"[\"'`]retries[\"'`]",
    r"\.retries\b",
    r"\(\s*retries\b",
    r",\s*retries\s*[,)]",
]


class LeftoversTest(unittest.TestCase):
    def shipped_files(self):
        for top in ("fetchkit", "scripts", "config", "examples"):
            for folder, _, names in os.walk(os.path.join(ROOT, top)):
                for name in names:
                    if not name.endswith(".pyc"):
                        yield os.path.join(folder, name)
        yield os.path.join(ROOT, "README.md")

    def test_no_old_identifier_is_left(self):
        for path in self.shipped_files():
            with open(path, encoding="utf-8", errors="replace") as source:
                for number, line in enumerate(source, 1):
                    for pattern in OLD:
                        self.assertFalse(re.search(pattern, line), f"{os.path.relpath(path, ROOT)}:{number}: {line.strip()}")

    def test_readme_documents_the_new_names(self):
        with open(os.path.join(ROOT, "README.md"), encoding="utf-8") as source:
            readme = source.read()
        for name in ("--attempts", "FETCHKIT_ATTEMPTS"):
            self.assertIn(name, readme)


if __name__ == "__main__":
    unittest.main()
