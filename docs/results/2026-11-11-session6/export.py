#!/usr/bin/env python3
"""Export one markdown bundle per benchmark run, for grading.

export.py <results.json> <out-dir> [--first-repeat]

A bundle holds the task, what the hidden verification said, the agent's final message and its
diff against the task as shipped (hidden verifier files left out).
"""
import glob, json, os, subprocess, sys

REPO = "/tmp/hoplite/workspace"
res = json.load(open(sys.argv[1]))
out = sys.argv[2]
first_only = "--first-repeat" in sys.argv
os.makedirs(out, exist_ok=True)
bench = res["dir"]


def sh(args, cwd):
    return subprocess.run(args, cwd=cwd, capture_output=True, text=True, errors="replace").stdout


def clip(text, lines):
    rows = text.splitlines()
    return "\n".join(rows[:lines]) + (f"\n… ({len(rows) - lines} more lines)" if len(rows) > lines else "")


# session id -> (cwd, task text, final summary, finish status)
sessions = {}
prompts = {}
for f in glob.glob(bench + "/home/sessions/*/events.jsonl"):
    cwd = task = summary = status = evidence = ""
    for line in open(f, errors="replace"):
        try:
            e = json.loads(line)
        except Exception:
            continue
        t = e.get("type")
        if t == "session_start":
            cwd = e["cwd"]
        elif t == "user" and not task:
            task = e.get("text", "")
        elif t == "finish":
            summary, status, evidence = e.get("summary", ""), e.get("status", ""), e.get("evidence", "")
    sessions[os.path.basename(os.path.dirname(f))] = (cwd, task, summary, status, evidence)

n = 0
for r in res["runs"]:
    if first_only and r["repeat"] != 1:
        continue
    task, variant, sid = r["task"], r["variant"], r["session"] or ""
    hidden_dir = f"{REPO}/crates/sage-cli/bench/{task}/hidden"
    hidden = {os.path.relpath(os.path.join(d, f), hidden_dir).removesuffix(".txt") if f.endswith(".log.txt") else os.path.relpath(os.path.join(d, f), hidden_dir) for d, _, fs in os.walk(hidden_dir) for f in fs}
    if sid in sessions:
        cwd, text, summary, status, evidence = sessions[sid]
        prompts.setdefault(task, text)
        answer = f"finish status: {status} ({evidence})\n\n{summary}"
    else:
        # An external harness: its workspace is named like the session, its answer is the end of its output.
        cwd = f"{bench}/work/{sid}"
        log = f"{bench}/external/{sid}/output.log"
        answer = "(external harness; the end of its console output)\n\n" + (clip("\n".join(open(log, errors="replace").read().splitlines()[-45:]), 45) if os.path.exists(log) else "(no output kept)")
    if not os.path.isdir(cwd):
        continue
    sh(["git", "add", "-A", "-N", "."], cwd)
    names = [p for p in sh(["git", "diff", "HEAD", "--name-only"], cwd).splitlines() if p not in hidden and "__pycache__" not in p and not p.endswith(".pyc")]
    stat = sh(["git", "diff", "HEAD", "--stat", "--"] + names, cwd) if names else "(no files changed)"
    diff = sh(["git", "diff", "HEAD", "--"] + names, cwd) if names else ""
    body = f"""# {r['model']} · {task} · {variant} · repeat {r['repeat']}

- hidden verification: {'PASSED' if r['solved'] else 'FAILED'}{'' if r['solved'] else ' — ' + str(r.get('failure'))}
- turns: {r['stats']['turns']} · cost: ${r['stats']['cost']:.4f} · outcome reported by the harness: {r['outcome']}

## Task given to the agent

{prompts.get(task, '(see another bundle of this task)')}

## Verification output (tail)

```
{clip(r.get('verify_tail') or '', 40)}
```

## The agent's final message

{clip(answer, 60)}

## Files changed against the task as shipped

```
{clip(stat, 40)}
```

## Diff

```diff
{clip(diff, 320)}
```
"""
    name = f"{r['model'][:8]}__{task}__{variant.replace(':', '-').replace('+', '-')}__{r['repeat']}.md"
    open(os.path.join(out, name), "w").write(body)
    n += 1
print(f"{n} bundles in {out}")
