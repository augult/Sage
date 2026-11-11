#!/usr/bin/env python3
"""Compact trace of a Presage session: trace.py <events.jsonl> [max_arg_chars] [first_turn] [last_turn]"""
import json, sys

path = sys.argv[1]
width = int(sys.argv[2]) if len(sys.argv) > 2 else 110
lo = int(sys.argv[3]) if len(sys.argv) > 3 else 0
hi = int(sys.argv[4]) if len(sys.argv) > 4 else 10**9


def short(s, n=width):
    s = " ".join(str(s).split())
    return s if len(s) <= n else s[: n - 1] + "…"


turn = 0
for line in open(path):
    try:
        e = json.loads(line)
    except Exception:
        continue
    t = e.get("type")
    if "turn" in e:
        turn = e["turn"]
    if t == "session_start":
        print("SESSION", e.get("model"), e.get("mode"), "skills=", e.get("skills"))
        continue
    if t == "finish":
        print("FINISH", e.get("status"), "verified=", e.get("verified"), "|", short(e.get("evidence", "")), "|", short(e.get("summary", ""), 300))
        continue
    if not (lo <= turn <= hi):
        continue
    if t == "model":
        u = e.get("usage", {})
        print(f"T{turn} out={u.get('output')} rsn={u.get('reasoning')} prompt={u.get('prompt')} ms={e.get('ms')} fin={e.get('finish')} {short(e.get('content',''),70)!r}")
        for c in e.get("calls", []):
            print("  call", c.get("name"), short(c.get("arguments", "")))
    elif t == "step_end":
        print("  end ", e.get("tool"), e.get("status"), short(e.get("observation", "")))
    elif t == "note":
        print("  NOTE", e.get("kind"), short(e.get("text", ""), 200))
    elif t in ("effort", "decay", "error", "watch", "lesson"):
        keys = {k: v for k, v in e.items() if k not in ("seq", "ts", "type")}
        print(" ", t.upper(), short(json.dumps(keys, ensure_ascii=False), 200))
