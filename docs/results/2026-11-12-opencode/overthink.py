#!/usr/bin/env python3
"""What follows a reply cut for thinking too long.  usage: overthink.py <bench-dir>..."""
import glob
import json
import re
import sys

for root in sys.argv[1:]:
    for path in sorted(glob.glob(f'{root}/home/sessions/*/events.jsonl')):
        events = [json.loads(line) for line in open(path) if line.strip()]
        task = '?'
        cut_turn = []
        models = {}
        for e in events:
            t = e.get('type')
            if t == 'session_start':
                m = re.search(r'/work/\d+-(.+?)-(presage|no-review)', e.get('cwd', ''))
                task = m.group(1) if m else '?'
            elif t == 'note' and e.get('kind') == 'overthinking':
                cut_turn.append(e.get('turn'))
            elif t == 'model':
                u = e.get('usage', {})
                models[e['turn']] = (u.get('output', 0), u.get('reasoning', 0), e.get('ms', 0), [c.get('name') for c in e.get('calls', [])])
        if cut_turn:
            first = sorted(models)[:3]
            print(f'{task:<12} cut at turn {cut_turn} | next replies: ' + ' ; '.join(f'T{t} out={models[t][0]} rsn={models[t][1]} {models[t][2] / 1000:.0f}s {models[t][3]}' for t in first))
