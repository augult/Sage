"""Join the result files of the accounts that ran one arm into one file. usage: merge.py <out.json> <in.json> [...]"""
import json
import sys

out, runs, meta = sys.argv[1], [], None
for f in sys.argv[2:]:
    d = json.load(open(f))
    meta = meta or {k: v for k, v in d.items() if k in ("provider", "effort", "models", "simulated")}
    runs += d["runs"]
runs.sort(key=lambda r: (r["task"], r.get("repeat", 0)))
json.dump({**(meta or {}), "joined_from": len(sys.argv) - 2, "runs": runs}, open(out, "w"), indent=1)
print(out, len(runs), "runs,", sum(bool(r.get("solved")) for r in runs), "solved")
