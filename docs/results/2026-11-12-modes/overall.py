"""Overall solved/total per variant and model across stored results, and the tasks each variant failed."""
import collections
import glob
import json

tally = collections.defaultdict(lambda: [0, 0, collections.Counter(), 0])
for path in sorted(glob.glob("docs/results/**/*.json", recursive=True)):
    try:
        rep = json.load(open(path))
    except Exception:
        continue
    for r in rep.get("runs", []):
        if r.get("error"):
            continue
        key = (r.get("model", "?")[:12], r.get("variant"))
        t = tally[key]
        t[1] += 1
        t[0] += bool(r.get("solved"))
        if not r.get("solved"):
            t[2][r.get("task")] += 1
            # Said done and verified by the harness, yet the hidden check failed.
            t[3] += bool(r.get("outcome") == "done")
for (model, variant), (solved, total, failed, false_done) in sorted(tally.items(), key=lambda kv: (kv[0][0], -kv[1][1])):
    if total < 8:
        continue
    print(f"{model:12} {variant:28} {solved:3}/{total:3}  said-done-but-failed {false_done:2}  failed: {dict(failed.most_common(6))}")
