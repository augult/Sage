#!/usr/bin/env python3
"""Which reasoning controls does the gateway honour for a model? effort_probe.py <model>"""
import json, sys, time, urllib.request

key = open("/tmp/k/logfare").read().strip()
model = sys.argv[1]
prompt = "Write a Python unittest file with 12 precise tests for a function `clamp(x, lo, hi)` that raises ValueError when lo > hi. Think about every edge case."
variants = {
    "nothing": {},
    "reasoning.effort=high": {"reasoning": {"effort": "high"}},
    "reasoning.effort=low": {"reasoning": {"effort": "low"}},
    "reasoning.enabled=false": {"reasoning": {"enabled": False}},
    "reasoning.max_tokens=2000": {"reasoning": {"max_tokens": 2000}},
    "thinking.disabled": {"thinking": {"type": "disabled"}},
    "reasoning_effort=low": {"reasoning_effort": "low"},
    "reasoning_effort=none": {"reasoning_effort": "none"},
}
if len(sys.argv) > 2 and sys.argv[2] == "combo":
    variants = {
        "A high both": {"reasoning": {"effort": "high"}, "include_reasoning": True, "reasoning_effort": "high"},
        "B off both": {"reasoning": {"enabled": False}, "reasoning_effort": "low"},
        "C low both": {"reasoning": {"effort": "low"}, "include_reasoning": True, "reasoning_effort": "low"},
        "D max both": {"reasoning": {"effort": "max"}, "include_reasoning": True, "reasoning_effort": "max"},
    }
elif len(sys.argv) > 2:
    variants = {f"reasoning_effort={v}": {"reasoning_effort": v} for v in sys.argv[2:]}
for name, extra in variants.items():
    body = {"model": model, "messages": [{"role": "user", "content": prompt}], "max_tokens": 6000, **extra}
    req = urllib.request.Request("https://logfare.ai/v1/chat/completions", data=json.dumps(body).encode(), headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"})
    t = time.time()
    try:
        with urllib.request.urlopen(req, timeout=240) as r:
            d = json.load(r)
        u = d.get("usage", {})
        details = u.get("completion_tokens_details") or {}
        msg = d["choices"][0]["message"]
        rc = msg.get("reasoning_content") or msg.get("reasoning") or ""
        print(f"{name:28} out={u.get('completion_tokens')} reasoning_tokens={details.get('reasoning_tokens')} reasoning_chars={len(rc)} content_chars={len(msg.get('content') or '')} {time.time()-t:.0f}s")
    except Exception as e:
        detail = e.read().decode()[:160] if hasattr(e, "read") else str(e)[:160]
        print(f"{name:28} ERROR {detail}")
