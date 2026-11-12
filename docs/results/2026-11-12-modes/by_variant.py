"""Solved/total per variant for given tasks across the stored results: python3 by_variant.py task [task...]"""
import collections
import glob
import json
import sys

tasks = sys.argv[1:]
for task in tasks:
    tally = collections.defaultdict(lambda: [0, 0, [], []])
    for path in sorted(glob.glob("docs/results/**/*.json", recursive=True)):
        try:
            rep = json.load(open(path))
        except Exception:
            continue
        for r in rep.get("runs", []):
            if r.get("task") != task or r.get("error"):
                continue
            model = r.get("model", "?")
            key = (model[:14], r.get("variant"))
            t = tally[key]
            t[1] += 1
            t[0] += bool(r.get("solved"))
            t[2].append(r.get("stats", {}).get("turns", 0))
            t[3].append(r.get("stats", {}).get("elapsed_ms", 0) / 1000)
    print(f"== {task}")
    for (model, variant), (solved, total, turns, secs) in sorted(tally.items(), key=lambda kv: (kv[0][0], -kv[1][1])):
        turns.sort()
        secs.sort()
        print(f"  {model:14} {variant:32} {solved:2}/{total:2}  median {turns[len(turns) // 2]:2} turns {secs[len(secs) // 2]:4.0f} s")
