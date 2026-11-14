"""Which runs went to the network for the exercise's tests or solution.

usage: webuse.py prosage <arm dir> [...]   |   webuse.py opencode <arm dir> [...]
"""
import glob
import json
import os
import re
import sys

kind, arms = sys.argv[1], sys.argv[2:]
FETCH = re.compile(r"\b(curl|wget)\b|urllib|urlopen|requests\.get|http\.client|fetch\(|pip3? install|npm (install|i)\b|git clone", re.I)
total, hits = 0, []
for arm in arms:
    if kind == "opencode":
        for log in sorted(glob.glob(os.path.join(arm, "home", "bench", "*", "external", "*", "output.log"))):
            total += 1
            text = re.sub(r"\x1b\[[0-9;]*m", "", open(log, errors="replace").read())
            lines = [l.strip() for l in text.splitlines() if re.match(r"\s*%\s*(WebFetch|WebSearch)", l) or (l.lstrip().startswith("$") and FETCH.search(l))]
            if lines:
                hits.append((os.path.basename(os.path.dirname(log)), lines))
    else:
        for ev in sorted(glob.glob(os.path.join(arm, "home", "bench", "*", "home", "sessions", "*", "events.jsonl"))):
            cwd, lines, done = "", [], False
            for line in open(ev):
                try:
                    e = json.loads(line)
                except Exception:
                    continue
                if e.get("type") == "session_start":
                    cwd = e.get("cwd", "")
                elif e.get("type") == "finish":
                    done = True
                elif e.get("type") == "model":
                    for c in e.get("calls") or []:
                        if c.get("name") == "shell":
                            try:
                                cmd = json.loads(c.get("arguments") or "{}").get("cmd", "")
                            except Exception:
                                cmd = c.get("arguments", "")
                            if FETCH.search(cmd):
                                lines.append(cmd.replace("\n", " ")[:160])
            if not done:
                continue
            total += 1
            if lines:
                hits.append((os.path.basename(cwd), lines))
print(f"{kind}: {len(hits)} of {total} runs went to the network")
for name, lines in hits[: int(os.environ.get("SHOW", "6"))]:
    print(f"  {name}: {lines[0][:170]}")
if os.environ.get("NAMES"):
    print(" ".join(sorted(n for n, _ in hits)))
