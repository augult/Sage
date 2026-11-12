#!/usr/bin/env python3
"""Join the runs of several result files into one, so that an.py and cmp.py see them as repeats.
usage: merge.py <out.json> <in.json>..."""
import json
import sys

out, inputs = sys.argv[1], sys.argv[2:]
merged = None
for n, path in enumerate(inputs, 1):
    d = json.load(open(path))
    for r in d['runs']:
        r['repeat'] = n
        r['source'] = path.split('/')[-1]
    if merged is None:
        merged = d
        merged['summary'] = []
    else:
        merged['runs'].extend(d['runs'])
        for v in d.get('variants', []):
            if v not in merged.get('variants', []):
                merged.setdefault('variants', []).append(v)
json.dump(merged, open(out, 'w'))
print(f'{out}: {len(merged["runs"])} runs from {len(inputs)} files')
