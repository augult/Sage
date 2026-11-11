#!/usr/bin/env python3
"""Where the turns go. waste.py <bench-dir>... [--model glm] [--mode presage]"""
import collections, glob, json, re, sys

roots = [a for a in sys.argv[1:] if not a.startswith("--")]
opt = dict(zip(sys.argv[1:], sys.argv[2:]))
want_model, want_mode = opt.get("--model", ""), opt.get("--mode", "")

tot = collections.Counter()
viol = collections.Counter()
errs = collections.Counter()
single = collections.Counter()
fin = collections.Counter()
notes = collections.Counter()
n = 0
for root in roots:
    for f in glob.glob(root.rstrip("/") + "/home/sessions/*/events.jsonl"):
        ev = []
        for line in open(f):
            try:
                ev.append(json.loads(line))
            except Exception:
                pass
        start = next((e for e in ev if e.get("type") == "session_start"), None)
        if not start or want_model not in start["model"] or (want_mode and start["mode"] != want_mode):
            continue
        if not any(e.get("type") == "finish" for e in ev):
            continue  # still running
        n += 1
        turns = [e for e in ev if e.get("type") == "model"]
        tot["turns"] += len(turns)
        last_change = first_finish = 0
        reads = collections.Counter()
        for e in ev:
            t = e.get("type")
            if t == "model":
                calls = e.get("calls", [])
                if len(calls) == 1:
                    single[calls[0]["name"]] += 1
                if len(calls) == 0:
                    single["(none)"] += 1
                for c in calls:
                    tot["call_" + c["name"]] += 1
                    if c["name"] in ("edit", "write"):
                        last_change = e["turn"]
                    if c["name"] == "finish" and not first_finish:
                        first_finish = e["turn"]
                    if c["name"] == "read":
                        try:
                            a = json.loads(c["arguments"])
                            reads[(a.get("path"), str(a.get("lines")))] += 1
                        except Exception:
                            pass
                u = e.get("usage", {})
                tot["reasoning"] += u.get("reasoning", 0)
                tot["output"] += u.get("output", 0)
            elif t == "step_end":
                st = e.get("status")
                o = e.get("observation", "")
                if st == "surprise":
                    for v in e.get("violations", []) or ["?"]:
                        viol[re.sub(r"\d+", "N", v)[:70]] += 1
                elif st == "error":
                    errs[e.get("tool", "?") + ": " + re.sub(r"[\d/]+", "N", o)[:75]] += 1
                elif st == "skipped":
                    tot["skipped"] += 1
                if e.get("tool") == "finish":
                    fin[o[:40]] += 1
            elif t == "note":
                notes[e.get("kind", "") + ": " + e.get("text", "")[:60]] += 1
        tot["turns_after_last_change"] += len(turns) - last_change if last_change else 0
        tot["turns_after_first_finish"] += len(turns) - first_finish if first_finish else 0
        tot["never_called_finish"] += first_finish == 0
        tot["repeat_reads"] += sum(c - 1 for c in reads.values() if c > 1)

print(f"sessions={n} turns={tot['turns']} mean={tot['turns']/max(n,1):.1f}")
print("per session:", {k: round(v / max(n, 1), 1) for k, v in sorted(tot.items()) if k != "turns"})
print("single-call turns by tool:", dict(single.most_common(8)))
for name, c in (("SURPRISES", viol), ("ERRORS", errs), ("FINISH ANSWERS", fin), ("NOTES", notes)):
    print(name)
    for k, v in c.most_common(9):
        print(f"  {v:3} {k}")
