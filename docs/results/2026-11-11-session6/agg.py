#!/usr/bin/env python3
"""Aggregate Sage session logs of bench runs.

agg.py <bench-dir>... [--rows]   bench-dir is e.g. target/scratch/ph/bench/20261111-...
Prints per (model, variant) means, and with --rows one line per session.
"""
import collections, glob, json, re, sys

roots = [a for a in sys.argv[1:] if not a.startswith("--")]
rows = "--rows" in sys.argv


def variant_of(cwd):
    # work/<n>-<task>-<variant>-<rep>; task ids may contain a hyphen (audit-clean, deadcode-js)
    name = cwd.rstrip("/").split("/")[-1]
    m = re.match(r"\d+-(.+?)-((?:ecosage|classic|no-[a-z-]+?)(?:\+[^-]+(?:-[a-z]+)?)*)-(\d+)$", name)
    return (m.group(1), m.group(2)) if m else (name, "?")


S = []
for root in roots:
    for f in sorted(glob.glob(root.rstrip("/") + "/home/sessions/*/events.jsonl")):
        d = collections.Counter()
        model = task = var = status = ""
        for line in open(f):
            try:
                e = json.loads(line)
            except Exception:
                continue
            t = e.get("type")
            if t == "session_start":
                model = e["model"][:8]
                task, var = variant_of(e["cwd"])
            elif t == "model":
                d["turns"] += 1
                n = len(e.get("calls", []))
                d["calls"] += n
                d["single"] += n == 1
                u = e.get("usage", {})
                d["out"] += u.get("output", 0)
                d["reason"] += u.get("reasoning", 0)
                d["ms"] += e.get("ms", 0)
                d["length"] += e.get("finish") == "length"
                d["repairs"] += e.get("repairs", 0)
            elif t == "step_end":
                d["st_" + str(e.get("status"))] += 1
                o = e.get("observation", "")
                if e.get("tool") == "finish":
                    d["gate"] += o.startswith("[gate]")
                    d["review"] += o.startswith("[review]")
                    d["wref"] += o.startswith("[SURPRISE] finish refused")
                d["tool_ms"] += e.get("ms", 0)
            elif t == "finish":
                status = e.get("status")
            elif t == "effort":
                d["eff"] += 1
        if d["turns"]:
            S.append((model, var, task, status, d))

if rows:
    for model, var, task, status, d in S:
        print(f"{model:9}{var:15}{task:12} {status:8} T={d['turns']:2} calls={d['calls']:3} single={d['single']:2} sur={d['st_surprise']:2} err={d['st_error']:2} gate={d['gate']} rev={d['review']} wref={d['wref']} eff={d['eff']} len={d['length']} out={d['out']//1000}k model_s={d['ms']//1000} tool_s={d['tool_ms']//1000}")

G = collections.defaultdict(list)
for model, var, task, status, d in S:
    G[(model, var)].append((status, d))
print(f"{'model':9}{'variant':15}{'n':>3} {'done':>4} {'budg':>4} {'turns':>5} {'calls/t':>7} {'single%':>7} {'surpr':>5} {'err':>4} {'gate':>4} {'rev':>4} {'wref':>4} {'out_k':>6} {'model_s':>7} {'tool_s':>6}")
for (model, var), items in sorted(G.items()):
    n = len(items)
    mean = lambda k: sum(d[k] for _, d in items) / n
    turns = sum(d["turns"] for _, d in items)
    print(f"{model:9}{var:15}{n:3} {sum(s == 'done' for s, _ in items):4} {sum(s == 'budget' for s, _ in items):4} {mean('turns'):5.1f} {sum(d['calls'] for _, d in items)/turns:7.2f} {100*sum(d['single'] for _, d in items)/turns:7.0f} {mean('st_surprise'):5.1f} {mean('st_error'):4.1f} {mean('gate'):4.1f} {mean('review'):4.1f} {mean('wref'):4.1f} {mean('out')/1000:6.1f} {mean('ms')/1000:7.0f} {mean('tool_ms')/1000:6.0f}")
