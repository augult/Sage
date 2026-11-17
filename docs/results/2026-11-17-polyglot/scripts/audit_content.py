"""Content-based leak audit: does a run's transcript contain lines of its task's hidden tests or reference solution
that are not in the workspace it was given? Also flags reads of real local sources (sage binary, /srv, hidden dirs).
usage: python3 -I audit_content.py [--list]"""
import glob, re, sys, pathlib, collections
ns = {"__name__": "tally"}
try: exec(open("/tmp/claude-0/-home-user-Sage/7cc046a5-3cd8-5b9a-989c-c3f9ed722f91/scratchpad/tally.py").read(), ns)
except SystemExit: pass
valid = ns["valid"]
B = pathlib.Path("/tmp/claude-0/-home-user-Sage/7cc046a5-3cd8-5b9a-989c-c3f9ed722f91/scratchpad/sage-pg/crates/sage-cli/bench")
def lines(d):
    out = set()
    for f in d.rglob("*"):
        if f.is_file():
            for l in f.read_text(errors="replace").splitlines():
                l = l.strip()
                if len(l) >= 30: out.add(l)
    return out
sig = {}
for t in B.glob("pgb-*"):
    ws = lines(t / "workspace")
    sig[t.name] = (lines(t / "hidden") - ws, lines(t / "solution") - ws)
SRC = re.compile(r"sagebench/sage[^\"\\n]{0,40}bench|/srv/part|/srv/btmp|/hidden/|sage-pg|polyglot-benchmark|scratchpad|bench/pgb-")
def norm(s): return s.replace('\\"', '"').replace("\\n", "\n").replace("\\t", "\t").replace("\\\\", "\\")
def scan(chunks, task):
    th, sh = sig[task]
    ft, fs, src = set(), set(), set()
    for c in chunks:
        t = norm(c)
        ft |= {l for l in th - ft if l in t}
        fs |= {l for l in sh - fs if l in t}
        src |= {m.group(0) for m in SRC.finditer(t)}
    return len(ft), len(th), len(fs), len(sh), sorted(src)
def flines(f):
    tail = ""
    with open(f, errors="replace") as h:
        while c := h.read(4 << 20):
            yield tail + c
            tail = c[-4000:]
res = collections.defaultdict(list)
for d in glob.glob("/srv/part*/sb-*/sage/bench/*"):
    if not re.search(r"/2026111[567]-(021309|051643|152715|001842|030852)-", d): continue  # the loop's own runs only; agent-made benches are separate
    d = pathlib.Path(d); part = "/" + "/".join(d.parts[1:3]); u = d.parts[3]
    for f in d.glob("external/*/output.log"):
        task = re.match(r"\d+-(pgb-.+?)-ext-", f.parent.name).group(1)
        if (u, task) in valid and valid[(u, task)][4] == part:
            res[(u, task)].append(scan(flines(f), task))
    for s in d.glob("home/sessions/*"):
        files = [p for p in sorted(s.rglob("*")) if p.is_file() and p.suffix in (".jsonl", ".log")]
        task = None
        for p in files:
            for c in flines(p):
                m = re.search(r'"cwd":"[^"]+/work/\d+-(pgb-.+?)-(?:pro|eco)sage-\d+', c)
                if m: task = m.group(1); break
            if task: break
        if task and (u, task) in valid and valid[(u, task)][4] == part:
            res[(u, task)].append(scan((c for p in files for c in flines(p)), task))
tab = collections.defaultdict(collections.Counter)
for (u, task), (var, ok, *_ ) in sorted(valid.items()):
    c = tab[var]; c["runs"] += 1; c["solved"] += ok
    r = res.get((u, task))
    if not r: c["nolog"] += 1; continue
    nt = max(x[0] for x in r); tt = r[0][1]; nsol = max(x[2] for x in r); ts = r[0][3]
    src = sorted({s for x in r for s in x[4]})
    leak = (tt and nt >= max(3, tt // 2)) or (ts and nsol >= max(3, ts // 2)) or any('bench' in x for x in src)
    c["leak"] += leak; c["leak_solved"] += leak and ok
    if "--list" in sys.argv and (leak or src or nt >= 5):
        print(f"{task:34} {var:16} {u:8} solved={ok!s:5} test-lines {nt}/{tt} sol-lines {nsol}/{ts} {src}")
for v, c in sorted(tab.items()):
    cl = c["runs"] - c["leak"] - c["nolog"]
    print(f"{v:16} runs {c['runs']:3} solved {c['solved']:3} | leak {c['leak']:3} (solved {c['leak_solved']}) | clean {cl} solved {c['solved']-c['leak_solved']} | no-log {c['nolog']}")
