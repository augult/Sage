"""Two arms by task: where each one solved what the other did not. usage: versus.py A.json[,A2.json] B.json[,B2.json] [nameA nameB]"""
import collections
import json
import sys


def load(spec):
    rows = collections.defaultdict(list)
    for f in spec.split(","):
        for r in json.load(open(f))["runs"]:
            rows[r["task"]].append(r)
    return rows


a, b = load(sys.argv[1]), load(sys.argv[2])
na, nb = (sys.argv[3], sys.argv[4]) if len(sys.argv) > 4 else ("A", "B")
both = neither = 0
only_a, only_b, mixed = [], [], []
for t in sorted(set(a) | set(b)):
    sa, sb = sum(bool(r.get("solved")) for r in a[t]), sum(bool(r.get("solved")) for r in b[t])
    la, lb = len(a[t]), len(b[t])
    if sa == la and sb == lb:
        both += 1
    elif sa == 0 and sb == 0:
        neither += 1
    elif sa > sb:
        only_a.append(f"{t} ({sa}/{la} against {sb}/{lb})")
    elif sb > sa:
        only_b.append(f"{t} ({sb}/{lb} against {sa}/{la})")
    else:
        mixed.append(f"{t} ({sa}/{la} each)")
g = lambda rows, k: sum((r.get("stats") or {}).get(k, 0) for rs in rows.values() for r in rs)
for name, rows in ((na, a), (nb, b)):
    n = sum(len(v) for v in rows.values())
    s = sum(bool(r.get("solved")) for v in rows.values() for r in v)
    print(f"{name}: {s}/{n} solved ({100 * s / n:.0f}%), requests {g(rows, 'turns') / n:.1f} + {g(rows, 'side_requests') / n:.1f} beside, {g(rows, 'elapsed_ms') / n / 1000:.0f} s and ${g(rows, 'cost') / n:.4f} a run, ${g(rows, 'cost'):.2f} in all")
print(f"tasks: {len(set(a) | set(b))}; every run solved by both: {both}; no run solved by either: {neither}")
print(f"{na} ahead on {len(only_a)}: " + "; ".join(only_a))
print(f"{nb} ahead on {len(only_b)}: " + "; ".join(only_b))
print(f"level, neither all nor none ({len(mixed)}): " + "; ".join(mixed))
