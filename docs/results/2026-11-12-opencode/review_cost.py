#!/usr/bin/env python3
"""Per-session cost of the review pass, of overthinking and of refused writes.
usage: review_cost.py <bench-dir> [--rows]"""
import glob
import json
import re
import sys

root = sys.argv[1]
rows = '--rows' in sys.argv
tot = dict(sessions=0, turns=0, after_review=0, overthink=0, overthink_sessions=0, refused_writes=0, out=0, rsn=0, rsn_first=0, anchor_err=0, solo=0, effort_low=0, edits_after_review=0, sessions_edit_after_review=0, big_rsn_turns=0)
for path in sorted(glob.glob(f'{root}/home/sessions/*/events.jsonl')):
    events = [json.loads(line) for line in open(path) if line.strip()]
    task = '?'
    turns = 0
    first_finish = None
    over = 0
    refused = 0
    out = rsn = rsn_first = 0
    anchor = 0
    solo = 0
    low = False
    edits_after = 0
    big = 0
    for e in events:
        t = e.get('type')
        if t == 'session_start':
            m = re.search(r'/work/\d+-(.+)-presage', e.get('cwd', ''))
            task = m.group(1) if m else '?'
        elif t == 'model':
            turns = e['turn']
            u = e.get('usage', {})
            out += u.get('output', 0)
            rsn += u.get('reasoning', 0)
            if u.get('reasoning', 0) >= 4000:
                big += 1
            if turns == 1:
                rsn_first = u.get('reasoning', 0)
            calls = e.get('calls', [])
            if len(calls) == 1:
                solo += 1
            if first_finish is not None and any(c.get('name') in ('edit', 'write') for c in calls):
                edits_after += 1
        elif t == 'note' and e.get('kind') == 'overthinking':
            over += 1
        elif t == 'effort' and e.get('to') == 'low':
            low = True
        elif t == 'step_end':
            obs = e.get('observation', '')
            if e.get('tool') == 'finish' and obs.startswith('[review]') and first_finish is None:
                first_finish = e['turn']
            if e.get('tool') == 'write' and 'already exists' in obs:
                refused += 1
            if e.get('tool') == 'edit' and 'no longer matches' in obs:
                anchor += 1
        elif t == 'stats':
            turns = max(turns, e.get('stats', {}).get('turns', 0)) if isinstance(e.get('stats'), dict) else turns
    after = turns - first_finish if first_finish is not None else 0
    tot['sessions'] += 1
    tot['turns'] += turns
    tot['after_review'] += after
    tot['overthink'] += over
    tot['overthink_sessions'] += 1 if over else 0
    tot['refused_writes'] += refused
    tot['out'] += out
    tot['rsn'] += rsn
    tot['rsn_first'] += rsn_first
    tot['anchor_err'] += anchor
    tot['solo'] += solo
    tot['effort_low'] += 1 if low else 0
    tot['edits_after_review'] += edits_after
    tot['sessions_edit_after_review'] += 1 if edits_after else 0
    tot['big_rsn_turns'] += big
    if rows:
        print(f'{task:<12} turns {turns:>2} first-finish@{first_finish} after {after} overthink {over} refused-write {refused} anchor-err {anchor} out {out} rsn {rsn} edits-after-review {edits_after}')
print(tot)
