#!/usr/bin/env python3
"""One line per turn of a Presage session.  usage: turns.py <events.jsonl> [arg chars]"""
import json
import sys

width = int(sys.argv[2]) if len(sys.argv) > 2 else 46


def short(s, n=width):
    s = ' '.join(str(s).split())
    return s if len(s) <= n else s[:n - 1] + '…'


def arg(call):
    try:
        a = json.loads(call.get('arguments') or '{}')
    except Exception:
        return short(call.get('arguments', ''))
    for key in ('cmd', 'path', 'pattern', 'summary', 'name'):
        if key in a:
            extra = f' L{a["lines"]}' if 'lines' in a else ''
            return short(a[key]) + extra
    return short(json.dumps(a))


notes = []
for line in open(sys.argv[1]):
    try:
        e = json.loads(line)
    except Exception:
        continue
    t = e.get('type')
    if t == 'model':
        u = e.get('usage', {})
        calls = ' | '.join(f'{c.get("name")}({arg(c)})' for c in e.get('calls', [])) or f'PROSE {short(e.get("content", ""), 60)!r}'
        print(f'T{e["turn"]:<2} {e.get("ms", 0) / 1000:5.1f}s out={u.get("output", 0):<5} rsn={u.get("reasoning", 0):<5} {calls}')
    elif t == 'step_end' and e.get('status') != 'ok':
        print(f'      !{e.get("tool")} {e.get("status")}: {short(e.get("observation", ""), 110)}')
    elif t == 'note':
        print(f'      NOTE {e.get("kind")}: {short(e.get("text", ""), 110)}')
    elif t == 'effort':
        print(f'      EFFORT {e.get("from")} -> {e.get("to")}: {e.get("reason")}')
    elif t == 'watch' and e.get('change') not in ('same', None):
        print(f'      WATCH {e.get("state")} ({e.get("change")}) {short(e.get("cmd", ""), 50)}')
    elif t == 'finish':
        print(f'FINISH {e.get("status")} verified={e.get("verified")} {short(e.get("evidence", ""), 60)}')
