"""Per-task comparison of two sets of result files. usage: pertask.py A1.json[,A2.json] B1.json[,B2.json]"""
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
g = lambda rs, k: sum((r.get("stats") or {}).get(k, 0) for r in rs) / max(len(rs), 1)
print(f"{'task':<13} {'solved':>9} {'turns':>11} {'beside':>9} {'out tokens':>15} {'cost/run':>17} {'secs':>9}")
ta = tb = 0.0
for t in sorted(set(a) | set(b)):
    ra, rb = a.get(t, []), b.get(t, [])
    sa, sb = sum(bool(r.get("solved")) for r in ra), sum(bool(r.get("solved")) for r in rb)
    ca, cb = g(ra, "cost"), g(rb, "cost")
    ta += ca * len(ra)
    tb += cb * len(rb)
    print(
        f"{t:<13} {sa}/{len(ra)} {sb}/{len(rb):<3} {g(ra, 'turns'):>5.1f} {g(rb, 'turns'):>5.1f} {g(ra, 'side_requests'):>4.1f} {g(rb, 'side_requests'):>4.1f} {g(ra, 'output_tokens') / 1000:>7.1f}k {g(rb, 'output_tokens') / 1000:>6.1f}k {ca:>8.4f} {cb:>8.4f} {g(ra, 'elapsed_ms') / 1000:>4.0f} {g(rb, 'elapsed_ms') / 1000:>4.0f}"
    )
na, nb = sum(len(v) for v in a.values()), sum(len(v) for v in b.values())
print(f"total: ${ta:.3f} over {na} runs (${ta / na:.4f}/run)  ->  ${tb:.3f} over {nb} runs (${tb / nb:.4f}/run)  = {100 * (tb / nb - ta / na) / (ta / na):+.0f}%")
