"""Tally isolated blind-run results from the per-account logs in /srv/glm/part*/<user>/main*.txt.
usage: python3 -I tally.py [--remaining]   (writes /srv/glm/partN+1/<user>.remaining with --remaining)"""
import glob, re, sys, collections
USERS = ["sb-eco1","sb-eco2","sb-oc1","sb-oc2"]
TASKS = re.findall(r'id: "(pgb-[^"]+)"', open("/tmp/claude-0/-home-user-Sage/7cc046a5-3cd8-5b9a-989c-c3f9ed722f91/scratchpad/sage-pg/crates/sage-cli/src/bench/tasks/polyblind.rs").read())
LINE = re.compile(r"· (pgb-\S+) · (\S+) #1: (solved|NOT solved \(([^)]*)\)?|no result)[^·]*(?:·[^·]*)*?· (\d+) turns · \$([\d.]+) · (\S+)")
INFRA = re.compile(r"no result|error: .*(unavailable|rate limit|429|503|budget|did not respond|capacity|temporarily)", re.I)
def secs(t):
    s = 0
    for v, u in re.findall(r"([\d.]+)(h|m|s)", t): s += float(v) * {"h":3600,"m":60,"s":1}[u]
    return s
EXCL = set(tuple(l.split()) for l in open("/srv/excluded.txt")) if __import__("os").path.exists("/srv/excluded.txt") else set()
valid = {}  # (user, task) -> (variant, solved, cost, secs)
for part in sorted(glob.glob("/srv/glm/part*")):
    for u in USERS:
        for f in sorted(glob.glob(f"{part}/{u}/glm*.txt")):
            if f.endswith("main-part1.txt"): continue
            for l in open(f, errors="replace"):
                m = LINE.search(l)
                if not m: continue
                task, var, status, why, turns, cost, t = m.groups()
                if INFRA.search(m.group(3) + " " + l.split("#1:")[1][:200]) and not status == "solved":
                    continue
                valid[(u, task)] = (var, status == "solved", float(cost), secs(t), part)
if __name__ != "__main__": pass
elif "--remaining" in sys.argv:
    nxt = sys.argv[sys.argv.index("--remaining") + 1]
    for u in USERS:
        left = [t for t in TASKS if (u, t) not in valid]
        open(f"{nxt}/{u}.remaining", "w").write(",".join(left))
        print(u, "valid", sum(1 for k in valid if k[0] == u), "left", len(left))
if __name__ != "__main__": raise SystemExit
agg = collections.defaultdict(lambda: collections.Counter())
for (u, task), (var, ok, cost, s, _p) in valid.items():
    lang = task.split("-")[1]
    for k in (lang, "all"):
        c = agg[(var, k)]; c["runs"] += 1; c["solved"] += ok; c["cost"] += cost; c["secs"] += s
for (var, k), c in sorted(agg.items()):
    print(f"{var:16} {k:4} {c['solved']:3}/{c['runs']:<3} ${c['cost']:.3f} avg {c['secs']/c['runs']:.0f}s")
