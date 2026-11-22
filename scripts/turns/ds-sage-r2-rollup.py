THOUGHT = r'''Audit the README sentence by sentence against the file on disk, then run one self-contained check covering what check.sh misses, and finish: report fields and formats, validation rules (spacing, method, path, three-digit status, ms, b), normalisation of "/" and trailing slashes, percentiles and ties, sorting, (other), CLI forms and exit 2, stdin parity, empty report.'''

CONTENT = r'''Auditing the README rules against `rollup.sh` and running the edge-case suite once more, including "/" normalisation, a 999 status, and `-n 1`.'''

CMD = r'''sh check.sh && python3 - <<'PY'
import subprocess, tempfile, os, sys

fails = []

def run(args, stdin=""):
    p = subprocess.run(["sh", "rollup.sh"] + args, input=stdin, text=True, capture_output=True)
    return p.returncode, p.stdout, p.stderr

def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name)
    if not ok:
        fails.append(name + ((": " + detail) if detail else ""))

HDR = "%-32s %7s %6s %6s %6s %10s" % ("endpoint", "count", "err%", "p50", "p95", "bytes")

def row(ep, c, pct, p50, p95, b):
    return "%-32s %7s %6.1f %6s %6s %10s" % (ep, c, pct, p50, p95, b)

def report(requests, skipped, window, slowest, rows):
    out = "requests: %d\nskipped: %d\nwindow: %s\n" % (requests, skipped, window)
    if slowest:
        out += "slowest: %s\n\n%s\n" % (slowest, HDR)
        out += "".join(r + "\n" for r in rows)
    return out

d = tempfile.mkdtemp()

def write(name, text):
    p = os.path.join(d, name)
    open(p, "w").write(text)
    return p

A = "\n".join([
    "2026-03-02T10:15:02Z GET /api/users/42/?verbose=1 200 10ms 100b",
    "2026-03-02T10:15:03Z GET /api/users/7 500 20ms 200b",
    "",
    "2026-03-02T10:15:04Z HEAD /api 200 5ms 5b",
    "2026-03-02T10:15:04Z GET api 200 5ms 5b",
    "2026-03-02T10:15:04Z GET /api 20 5ms 5b",
    "2026-03-02T10:15:04Z GET /api 200 5 5b",
    "2026-03-02T10:15:04Z GET /api 200 5ms 5",
    "2026-03-02T10:15:04Z  GET /api 200 5ms 5b",
    "2026-03-02T10:15:04Z GET  /api 200 5ms 5b",
    "2026-03-02T10:15:04Z GET /api  200 5ms 5b",
    "2026-03-02T10:15:04Z GET /api 200 5ms 5b ",
]) + "\n"
pa = write("a.log", A)
expA = report(2, 9, "2026-03-02T10:15:02Z .. 2026-03-02T10:15:03Z",
              "20ms GET /api/users/7 at 2026-03-02T10:15:03Z",
              [row("GET /api/users/:id", "2", 50.0, "10", "20", "300")])
rc, out, _ = run([pa])
check("six fields, spacing, method, status, ms, b; blanks not counted", rc == 0 and out == expA, repr(out))

B = "\n".join([
    "2026-03-02T10:00:00Z GET /b 200 30ms 1b",
    "2026-03-02T10:00:00Z GET /a 200 10ms 1b",
    "2026-03-02T10:00:00Z GET /a 200 20ms 1b",
    "2026-03-02T10:00:00Z GET /a 500 20ms 2b",
    "2026-03-02T10:00:00Z GET /c 404 40ms 5b",
]) + "\n"
pb = write("b.log", B)
expB = report(5, 0, "2026-03-02T10:00:00Z .. 2026-03-02T10:00:00Z",
              "40ms GET /c at 2026-03-02T10:00:00Z",
              [row("GET /a", "3", 100.0 / 3, "20", "20", "4"),
               row("GET /b", "1", 0.0, "30", "30", "1"),
               row("(other)", "1", 0.0, "-", "-", "5")])
rc, out, _ = run(["-n", "2", pb])
check("count sort, endpoint tie, err%, (other) row", rc == 0 and out == expB, repr(out))

rc, out, _ = run(["-n", "9", pb])
check("no (other) when N covers every endpoint", rc == 0 and "(other)" not in out, repr(out))

rc, out, _ = run(["-n", "1", pb])
check("-n 1 leaves one row plus (other)",
      rc == 0 and out.splitlines()[-2].startswith("GET /a") and out.splitlines()[-1].startswith("(other)"),
      repr(out))

C = "\n".join([
    "2026-03-02T10:09:00Z GET /first 200 50ms 1b",
    "2026-03-02T10:01:00Z GET /second 200 50ms 1b",
    "2026-03-02T10:05:00Z GET /mid 200 7ms 1b",
]) + "\n"
pc = write("c.log", C)
rc, out, _ = run([pc])
check("window min/max, slowest tie takes first in file",
      rc == 0
      and "window: 2026-03-02T10:01:00Z .. 2026-03-02T10:09:00Z\n" in out
      and "slowest: 50ms GET /first at 2026-03-02T10:09:00Z\n" in out, repr(out))

longep = "GET /a/very/long/path/that/goes/past/thirtytwo"
pd_ = write("d.log", "2026-03-02T11:00:00Z " + longep + " 200 5ms 1b\n")
rc, out, _ = run([pd_])
check("endpoint longer than 32 chars printed in full", rc == 0 and out.splitlines()[-1].startswith(longep), repr(out))

pe = write("e.log", "\n\nbad\nstill bad\nnope\n")
rc, out, _ = run([pe])
check("no valid line report has no slowest or table",
      rc == 0 and out == "requests: 0\nskipped: 3\nwindow: -\n", repr(out))

pf = write("f.log", "2026-03-02T12:00:00Z GET /x 499 4ms 1b\n"
                    "2026-03-02T12:00:00Z GET /y 500 4ms 1b\n"
                    "2026-03-02T12:00:00Z GET /?a=1 999 0ms 0b\n")
rc, out, _ = run([pf])
xrow = [l for l in out.splitlines() if l.startswith("GET /x")]
yrow = [l for l in out.splitlines() if l.startswith("GET /y")]
root = [l for l in out.splitlines() if l.startswith("GET / ")]
check("err% boundary at 500", rc == 0 and xrow[0].split()[3] == "0.0" and yrow[0].split()[3] == "100.0", repr(out))
check("root path stays '/' and drops the query; 999ms status is an error",
      rc == 0 and root and root[0].split()[2] == "1" and root[0].split()[3] == "100.0", repr(out))

rc, fout, _ = run([pb])
rc2, sout, _ = run([], stdin=B)
rc3, dash, _ = run(["-"], stdin=B)
check("stdin and - match file input", rc == rc2 == rc3 == 0 and fout == sout == dash, repr(sout))

for args in (["-n", "0"], ["-n", "x"], ["-n"], ["-x"], ["a", "b"], ["-n", "3", "a", "b"], ["-n", "-2"]):
    rc, out, err = run(args, stdin="")
    check("usage error exit 2 for " + " ".join(args),
          rc == 2 and err.startswith("usage:") and out == "", repr((rc, out, err)))

print("EDGE OK" if not fails else "EDGE FAILED: " + "; ".join(fails))
sys.exit(1 if fails else 0)
PY'''

CALLS = [
    {"name": "shell", "arguments": {"cmd": CMD, "expect_exit": "0", "expect_has": "EDGE OK"}},
    {"name": "finish", "arguments": {"summary": "rollup.sh is complete against the README. Report: requests/skipped/window/slowest, then the blank line, header and rows in the stated widths; window is the min and max timestamp, slowest is the first request with the maximum latency using its path as logged, and an empty result prints only requests/skipped/window with '-'. Validation accepts exactly six single-space-separated fields with an allowed method, a leading '/', a three-digit status, NNNms and NNNb; blank lines are ignored and every other line is skipped, verified with leading/trailing/doubled spaces and each malformed field. Endpoints normalise by dropping the query, dropping one trailing slash (except '/'), and mapping digit-only segments to :id. Rows sort by count desc then endpoint in C order, err% is the share of statuses >= 500 at one decimal, p50/p95 use nearest rank over ascending latencies, bytes sum, and (other) aggregates the remainder with '-' percentiles. CLI: -n N (positive integer), FILE or stdin, '-' for stdin, anything else prints usage on stderr and exits 2. `sh check.sh` passes, plus the edge suite above.", "unverified": "Large logs, CRLF input and a mid-stream read failure were not run; (other) percentiles are '-' by design."}},
]
