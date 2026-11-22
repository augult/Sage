THOUGHT = "The first tool-call definition had a quoting error; I am simplifying the batch command and will inspect the resulting migration before completing it."
CONTENT = ""
CALLS = [
    {"name": "shell", "arguments": {"cmd": '''python3 - <<'PY'
from pathlib import Path
files = ['README.md','config/fetchkit.ini.template','examples/manifest.json','fetchkit/__init__.py','fetchkit/cli.py','fetchkit/client.py','fetchkit/config.py','fetchkit/jobs.py','fetchkit/retry.py','scripts/backfill.py','scripts/nightly.sh','scripts/smoke.sh']
for name in files:
 p=Path(name); s=p.read_text()
 for old,new in [('DEFAULT_RETRIES','DEFAULT_ATTEMPTS'),('PRIORITY_RETRIES','PRIORITY_ATTEMPTS'),('FETCHKIT_RETRIES','FETCHKIT_ATTEMPTS'),('retries','attempts'),('Retries','Attempts'),('RETRIES','ATTEMPTS')]: s=s.replace(old,new)
 p.write_text(s)
p=Path('fetchkit/retry.py'); s=p.read_text().replace('attempts=2','attempts=3').replace('range(attempts)]','range(max(0, attempts - 1))]').replace('attempts < 0','attempts < 1').replace('attempts must not be negative','attempts must be at least 1').replace('range(attempts + 1)','range(attempts)').replace('attempt == attempts','attempt == attempts - 1'); p.write_text(s)
p=Path('fetchkit/client.py'); s=p.read_text().replace('DEFAULT_ATTEMPTS = 2','DEFAULT_ATTEMPTS = 3').replace('attempts < 0','attempts < 1').replace('attempts must not be negative','attempts must be at least 1').replace('self.timeout * (self.attempts + 1)','self.timeout * self.attempts'); p.write_text(s)
p=Path('fetchkit/config.py'); s=p.read_text().replace('"attempts": 2','"attempts": 3').replace('attempts"] < 0','attempts"] < 1').replace('attempts must not be negative','attempts must be at least 1'); p.write_text(s)
p=Path('fetchkit/jobs.py'); s=p.read_text().replace('"low": 0, "normal": None, "high": 5','"low": 1, "normal": None, "high": 6').replace('attempts < 0','attempts < 1').replace('attempts must be a non-negative integer','attempts must be a positive integer'); p.write_text(s)
p=Path('examples/manifest.json'); s=p.read_text().replace('"attempts": 1','"attempts": 2').replace('"attempts": 0','"attempts": 1'); p.write_text(s)
p=Path('config/fetchkit.ini.template'); p.write_text(p.read_text().replace('attempts = 4','attempts = 5'))
p=Path('scripts/nightly.sh'); p.write_text(p.read_text().replace('--attempts 4','--attempts 5'))
p=Path('scripts/backfill.py'); p.write_text(p.read_text().replace('"attempts": 7','"attempts": 8').replace('fetch(INDEX, attempts=0)','fetch(INDEX, attempts=1)'))
p=Path('scripts/smoke.sh'); p.write_text(p.read_text().replace('FETCHKIT_ATTEMPTS=0','FETCHKIT_ATTEMPTS=1'))
p=Path('fetchkit/cli.py'); p.write_text(p.read_text().replace('default: 2','default: 3'))
p=Path('README.md'); s=p.read_text().replace('Client(attempts=3','Client(attempts=4').replace('attempts=0','attempts=1').replace('--attempts 1','--attempts 2').replace('"attempts" | 2','"attempts" | 3').replace('"attempts": 1','"attempts": 2').replace('four attempts per download','five attempts per download').replace('two retries','three attempts').replace('one try and no retries','one attempt').replace('five times','six attempts').replace('attempts = 4','attempts = 5').replace('the number of retries made','the number of attempts made'); p.write_text(s)
PY
''', "timeout": 120}},
    {"name": "read", "arguments": {"path": "fetchkit/retry.py"}},
    {"name": "read", "arguments": {"path": "fetchkit/client.py"}},
    {"name": "read", "arguments": {"path": "fetchkit/cli.py"}},
    {"name": "read", "arguments": {"path": "README.md"}},
]
