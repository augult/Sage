"""Combine bench result files: per variant solved, mean requests, time and cost; per task where a variant lost.

usage: bench_table.py LABEL=FILE[,FILE...] ...   (a label groups the runs of several files under one name per variant)
"""
import collections
import json
import sys

rows = collections.OrderedDict()
lost = collections.defaultdict(list)
for arg in sys.argv[1:]:
    label, files = arg.split('=', 1)
    for f in files.split(','):
        try:
            data = json.load(open(f))
        except Exception as e:
            print('cannot read', f, e)
            continue
        for r in data['runs']:
            name = f"{r['variant']}{label}"
            st = r.get('stats') or {}
            row = rows.setdefault(name, dict(n=0, solved=0, turns=0, side=0, ms=0, cost=0.0, err=0, times=[]))
            row['n'] += 1
            row['solved'] += bool(r.get('solved'))
            row['turns'] += st.get('turns', 0)
            row['side'] += st.get('side_requests', 0)
            row['ms'] += st.get('elapsed_ms', 0)
            row['times'].append(st.get('elapsed_ms', 0))
            row['cost'] += st.get('cost', 0.0)
            if not r.get('solved'):
                lost[name].append(f"{r['task']} ({r.get('outcome')}{', ' + str(r.get('error'))[:40] if r.get('error') else ''})")
print(f"{'variant':<28} {'solved':>7} {'requests':>9} {'beside':>7} {'mean time':>10} {'median':>7} {'cost':>8}")
for name, row in rows.items():
    n = row['n']
    med = sorted(row['times'])[n // 2] / 1000
    print(f"{name:<28} {row['solved']:>3}/{n:<3} {row['turns'] / n:>9.1f} {row['side'] / n:>7.1f} {row['ms'] / n / 1000:>9.0f}s {med:>6.0f}s {row['cost']:>7.2f}$")
for name, tasks in lost.items():
    print(f"lost by {name}: {'; '.join(tasks)}")
