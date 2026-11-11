#!/usr/bin/env python3
"""Rebuild a results file for a bench run that was stopped early, from its log and its folder.

salvage.py <log.txt> <bench-dir> <out.json>   keeps the runs whose result line was printed.
"""
import glob, json, os, re, sys

log, bench, out = sys.argv[1:4]
labels = {}  # (task, variant) -> session id or external label
for f in glob.glob(bench + "/home/sessions/*/events.jsonl"):
    first = json.loads(open(f, errors="replace").readline())
    m = re.match(r"\d+-(.+)-(\d+)$", first["cwd"].rstrip("/").split("/")[-1])
    if m:
        labels[m.group(1)] = os.path.basename(os.path.dirname(f))
for d in glob.glob(bench + "/external/*"):
    m = re.match(r"\d+-(.+)-(\d+)$", os.path.basename(d))
    if m:
        labels[m.group(1)] = os.path.basename(d)


def seconds(text):
    m = re.match(r"(?:(\d+)m)?([\d.]+)s", text)
    return (int(m.group(1) or 0) * 60 + float(m.group(2))) if m else 0.0


runs = []
model = ""
for line in open(log, errors="replace"):
    m = re.match(r"\[\d+/\d+\] (\S+) · (\S+) · (\S+) #(\d+): (solved|NOT solved(?: \((.*?)\))?|no result.*?) · (\d+) turns · \$([\d.]+) · (\S+)", line)
    if not m:
        continue
    model, task, variant = m.group(1), m.group(2), m.group(3)
    key = f"{task}-{variant.replace(':', '-')}"
    solved = m.group(5) == "solved"
    reason = m.group(6) or ""
    runs.append({
        "model": model, "task": task, "variant": variant, "repeat": int(m.group(4)), "session": labels.get(key, ""),
        "outcome": "done" if solved or reason == "done" else (reason.split(":")[0] or "error"), "solved": solved,
        "failure": None if solved else reason, "verify_tail": "", "error": None,
        "stats": {"turns": int(m.group(7)), "cost": float(m.group(8)), "elapsed_ms": int(seconds(m.group(9)) * 1000)},
    })
json.dump({"dir": os.path.abspath(bench), "models": [model], "salvaged_from": os.path.basename(log), "runs": runs}, open(out, "w"), indent=1)
print(f"{len(runs)} runs, {sum(1 for r in runs if not r['session'])} without a session")
