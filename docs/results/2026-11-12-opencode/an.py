#!/usr/bin/env python3
"""Summarise bench result files.  usage: an.py [--runs] [--calls] <results.json>..."""
import json
import statistics
import sys

args = [a for a in sys.argv[1:] if not a.startswith('--')]
flags = {a for a in sys.argv[1:] if a.startswith('--')}


def requests(run):
    """Model requests a run cost against the gateway's limits, failed ones included."""
    ext = run.get('external')
    if ext:
        u = ext['usage']
        return u['turns'] + u['failed']
    s = run['stats']
    return s['turns'] + s.get('retries', 0)


def tool_turns(run):
    """Requests that offered tools: leaves out a harness's title and summary requests."""
    ext = run.get('external')
    if not ext:
        return run['stats']['turns']
    return sum(1 for c in ext['usage']['calls'] if c['status'] == 200 and c['tools'] > 0)


for path in args:
    d = json.load(open(path))
    runs = d['runs']
    print(f'== {path}: {len(runs)} runs')
    by = {}
    for r in runs:
        by.setdefault((r['model'], r['variant']), []).append(r)
    for (model, variant), rs in by.items():
        counted = [r for r in rs if not r.get('error')]
        solved = sum(1 for r in counted if r['solved'])
        mean = lambda f: statistics.mean(f(r) for r in counted) if counted else 0
        med = lambda f: statistics.median(f(r) for r in counted) if counted else 0
        print(f'{model} {variant}: solved {solved}/{len(counted)} (provider errors {len(rs) - len(counted)})'
              f' | turns mean {mean(lambda r: r["stats"]["turns"]):.1f} med {med(lambda r: r["stats"]["turns"]):.0f}'
              f' | tool turns mean {mean(tool_turns):.1f}'
              f' | time mean {mean(lambda r: r["stats"]["elapsed_ms"]) / 1000:.0f}s med {med(lambda r: r["stats"]["elapsed_ms"]) / 1000:.0f}s'
              f' | cost sum ${sum(r["stats"]["cost"] for r in counted):.3f} mean ${mean(lambda r: r["stats"]["cost"]):.4f}'
              f' | no-cache sum ${sum(r.get("cost_uncached", 0) for r in counted):.3f}'
              f' | in tok mean {mean(lambda r: r["stats"]["prompt_tokens"]) / 1000:.1f}k out {mean(lambda r: r["stats"]["output_tokens"]) / 1000:.1f}k'
              f' | requests {sum(requests(r) for r in rs)}')
    if '--runs' in flags:
        for r in sorted(runs, key=lambda r: (r['task'], r['variant'], r['repeat'])):
            s = r['stats']
            note = r.get('error') or r.get('failure') or ''
            print(f'  {r["task"]:<12} {r["variant"]:<13} #{r["repeat"]} {"SOLVED" if r["solved"] else "unsolved":<8} {r["outcome"]:<7}'
                  f' turns {s["turns"]:>2} tool {tool_turns(r):>2} {s["elapsed_ms"] / 1000:>5.0f}s ${s["cost"]:.4f} in {s["prompt_tokens"] / 1000:>6.1f}k out {s["output_tokens"] / 1000:>5.1f}k'
                  f' {r["session"]} {note[:110]}')
    if '--calls' in flags:
        for r in runs:
            ext = r.get('external')
            if ext:
                u = ext['usage']
                print(r['task'], {k: v for k, v in u.items() if k != 'calls'})
                for c in u['calls']:
                    print('   ', c)
            else:
                print(r['task'], r['stats'])
