#!/usr/bin/env python3
"""Side by side per task.  usage: cmp.py <a.json> <b.json> [more.json...]"""
import json
import statistics
import sys

cols = []
for path in sys.argv[1:]:
    d = json.load(open(path))
    by = {}
    for r in d['runs']:
        by.setdefault((r['variant'], r['task']), []).append(r)
    for variant in dict.fromkeys(v for v, _ in by):
        cols.append((f'{path.split("/")[-1].replace(".json", "")}:{variant}', {t: rs for (v, t), rs in by.items() if v == variant}))

tasks = list(dict.fromkeys(t for _, by in cols for t in by))
print(f'{"task":<12}' + ''.join(f'{name[-26:]:>34}' for name, _ in cols))
tot = [[0, 0, 0, 0.0, 0.0] for _ in cols]
for t in tasks:
    row = f'{t:<12}'
    for i, (_, by) in enumerate(cols):
        rs = [r for r in by.get(t, []) if not r.get('error')]
        if not rs:
            row += f'{"-":>34}'
            continue
        solved = sum(r['solved'] for r in rs)
        turns = statistics.mean(r['stats']['turns'] for r in rs)
        secs = statistics.mean(r['stats']['elapsed_ms'] for r in rs) / 1000
        cost = statistics.mean(r['stats']['cost'] for r in rs)
        row += f'{solved}/{len(rs)} {turns:5.1f}t {secs:5.0f}s ${cost:.4f}'.rjust(34)
        tot[i][0] += solved
        tot[i][1] += len(rs)
        tot[i][2] += sum(r['stats']['turns'] for r in rs)
        tot[i][3] += sum(r['stats']['elapsed_ms'] for r in rs) / 1000
        tot[i][4] += sum(r['stats']['cost'] for r in rs)
    print(row)
row = f'{"ALL":<12}'
for s, n, turns, secs, cost in tot:
    row += f'{s}/{n} {turns / max(n, 1):5.1f}t {secs / max(n, 1):5.0f}s ${cost:.3f}'.rjust(34)
print(row)
