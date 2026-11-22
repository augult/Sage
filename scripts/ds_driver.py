import http.server
import json
import math
import os
import shutil
import socketserver
import subprocess
import sys
import threading
import time
from pathlib import Path

SAGE_BIN = "/tmp/hoplite/workspace/target/release/sage"
BASE_DIR = Path("/tmp/ds_runs")
TURNS_DIR = Path("/tmp/hoplite/workspace/scripts/turns")
PAGE_CHARS = 5000


def common_prefix_len(a: bytes, b: bytes) -> int:
    n = min(len(a), len(b))
    i = 0
    while i < n and a[i] == b[i]:
        i += 1
    return i


def format_tools(tools):
    out = []
    for t in tools:
        fn = t.get("function", {})
        name = fn.get("name", "")
        desc = (fn.get("description", "") or "").strip().splitlines()[0] if fn.get("description") else ""
        params = fn.get("parameters", {})
        props = params.get("properties", {})
        req = set(params.get("required", []))
        p_parts = []
        for k, v in props.items():
            star = "*" if k in req else ""
            tp = v.get("type", "any")
            p_parts.append(f"{k}{star}:{tp}")
        out.append(f"- {name}({', '.join(p_parts)}): {desc}")
    return "\n".join(out)


def render_turn_text(req_data) -> str:
    turn = req_data["turn"]
    lines = []
    if turn == 1:
        lines.append(f"=== TURN 1 (run_id={req_data['run_id']}, variant={req_data['variant']}, task={req_data['task']}) ===")
        for m in req_data["messages"]:
            role = m.get("role", "")
            content = m.get("content", "")
            if role == "system" and req_data["variant"] == "ext:opencode" and len(content) > 1600:
                content = content[:1600] + "\n...[standard OpenCode system instructions]..."
            lines.append(f"--- [{role.upper()}] ---\n{content}\n")
        lines.append("--- [TOOLS AVAILABLE IN THIS HARNESS] ---")
        lines.append(format_tools(req_data["tools"]))
    else:
        lines.append(f"=== TURN {turn} OBSERVATIONS (run_id={req_data['run_id']}) ===")
        for m in req_data["new_messages"]:
            role = m.get("role", "")
            tc_id = m.get("tool_call_id", "")
            content = m.get("content", "")
            lines.append(f"--- [{role.upper()} {tc_id}] ---\n{content}\n")
    return "\n".join(lines)


def queue_and_print_page(rdir: Path, run_id: str, text: str):
    if len(text) <= PAGE_CHARS:
        (rdir / "page_rest.txt").unlink(missing_ok=True)
        print(text)
        print(f"\n[END OF TURN OUTPUT. Edit scripts/turns/{run_id}.py with apply_patch and run: python3 scripts/ds_driver.py step {run_id}]")
    else:
        cut = text.rfind("\n", 0, PAGE_CHARS)
        if cut < PAGE_CHARS // 2:
            cut = PAGE_CHARS
        head = text[:cut]
        rest = text[cut:].lstrip("\n")
        (rdir / "page_rest.txt").write_text(rest)
        print(head)
        print(f"\n[PAGE BREAK ({len(rest)} chars remaining) — DO NOT REPLY YET! Run: python3 scripts/ds_driver.py more {run_id}]")


def more(run_id: str):
    rdir = BASE_DIR / run_id
    rest_file = rdir / "page_rest.txt"
    if not rest_file.exists():
        print(f"[No more pages. Edit scripts/turns/{run_id}.py with apply_patch and run: python3 scripts/ds_driver.py step {run_id}]")
        return
    queue_and_print_page(rdir, run_id, rest_file.read_text())


def run_daemon(run_id: str, task: str, variant: str, port: int):
    rdir = BASE_DIR / run_id
    rdir.mkdir(parents=True, exist_ok=True)
    shome = rdir / "sage_home"
    shome.mkdir(parents=True, exist_ok=True)
    cfg = {
        "keys": {"custom": "sk-local"},
        "provider": "custom",
        "model": "deepseek-v4.1-flash-1017",
        "custom": {
            "base_url": f"http://127.0.0.1:{port}/v1",
            "models": ["deepseek-v4.1-flash-1017"],
        },
    }
    (shome / "config.json").write_text(json.dumps(cfg))

    state = {
        "turn": 0,
        "prev_msg_count": 0,
        "prev_wire": b"",
        "lock": threading.Lock(),
    }

    class ThreadedHTTPServer(socketserver.ThreadingMixIn, http.server.HTTPServer):
        allow_reuse_address = True
        daemon_threads = True

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            ln = int(self.headers.get("Content-Length", 0))
            raw = self.rfile.read(ln)
            body = json.loads(raw)
            tools = body.get("tools", [])
            messages = body.get("messages", [])
            stream = bool(body.get("stream", True))

            if not tools and any("title" in str(m.get("content", "")).lower() for m in messages):
                pt = math.ceil(len(raw) / 4)
                self.send_reply(stream, "Task", "", [], pt, 0, 2, 0)
                return

            with state["lock"]:
                state["turn"] += 1
                turn = state["turn"]
                wire = json.dumps({"tools": tools, "messages": messages}, separators=(",", ":")).encode()
                common = common_prefix_len(wire, state["prev_wire"]) if state["prev_wire"] else 0
                state["prev_wire"] = wire
                prompt_tokens = math.ceil(len(wire) / 4)
                cached_tokens = min(prompt_tokens, (common // 256) * 64)
                new_msgs = messages[state["prev_msg_count"]:]

                resp_path = rdir / "turn_resp.json"
                if resp_path.exists():
                    resp_path.unlink()
                req_payload = {
                    "run_id": run_id,
                    "task": task,
                    "variant": variant,
                    "turn": turn,
                    "messages": messages if turn == 1 else [],
                    "new_messages": new_msgs,
                    "tools": tools if turn == 1 else [],
                    "asked_at": time.time(),
                }
                tmp_req = rdir / "turn_req.json.tmp"
                tmp_req.write_text(json.dumps(req_payload))
                tmp_req.replace(rdir / "turn_req.json")

                t0 = time.time()
                while not resp_path.exists():
                    if time.time() - t0 > 550:
                        self.send_error(504, "timeout waiting for reply")
                        return
                    time.sleep(0.05)

                ans = json.loads(resp_path.read_text())
                resp_path.unlink()
                think_sec = max(0.0, time.time() - t0)
                calls = ans.get("calls") or []
                content = ans.get("content") or ""
                thought = ans.get("thought") or ""

                state["prev_msg_count"] = len(messages) + 1

                vis_bytes = len(content.encode()) + len(json.dumps(calls).encode())
                vis_tokens = max(1, math.ceil(vis_bytes / 4))
                rsn_tokens = max(math.ceil(len(thought.encode()) / 4), int(think_sec * 25))
                out_tokens = vis_tokens + rsn_tokens

                self.send_reply(stream, content, thought, calls, prompt_tokens, cached_tokens, out_tokens, rsn_tokens)

        def send_reply(self, stream, content, thought, calls, pt, ct, ot, rt):
            usage = {
                "prompt_tokens": pt,
                "completion_tokens": ot,
                "total_tokens": pt + ot,
                "prompt_tokens_details": {"cached_tokens": ct},
                "completion_tokens_details": {"reasoning_tokens": rt},
            }
            tc_wire = []
            for idx, c in enumerate(calls):
                args = c.get("arguments", {})
                if not isinstance(args, str):
                    args = json.dumps(args)
                tc_wire.append({
                    "index": idx,
                    "id": f"call_{state['turn']}_{idx+1}",
                    "type": "function",
                    "function": {"name": c["name"], "arguments": args},
                })
            finish_reason = "tool_calls" if tc_wire else "stop"
            if stream:
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Cache-Control", "no-cache")
                self.end_headers()
                delta = {"role": "assistant", "content": content}
                if thought:
                    delta["reasoning_content"] = thought
                if tc_wire:
                    delta["tool_calls"] = tc_wire
                c1 = {
                    "id": f"chatcmpl-{state['turn']}",
                    "object": "chat.completion.chunk",
                    "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}],
                    "usage": usage,
                }
                self.wfile.write(f"data: {json.dumps(c1)}\n\ndata: [DONE]\n\n".encode())
            else:
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                msg = {"role": "assistant", "content": content}
                if thought:
                    msg["reasoning_content"] = thought
                if tc_wire:
                    msg["tool_calls"] = tc_wire
                resp = {
                    "id": f"chatcmpl-{state['turn']}",
                    "object": "chat.completion",
                    "choices": [{"index": 0, "message": msg, "finish_reason": finish_reason}],
                    "usage": usage,
                }
                self.wfile.write(json.dumps(resp).encode())

    srv = ThreadedHTTPServer(("127.0.0.1", port), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()

    env = dict(
        os.environ,
        SAGE_HOME=str(shome),
        BENCH_EXT_HOME="/home/hoplite/.local/share/sage-external",
        PATH=f"/home/hoplite/.local/share/sage-external/bin:/home/hoplite/.local/share/mise/shims:{os.environ.get('PATH', '')}",
    )
    out_json = rdir / "results.json"
    cmd = [
        SAGE_BIN,
        "bench",
        "--tasks",
        task,
        "--variants",
        variant,
        "-n",
        "1",
        "--max-turns",
        "30",
        "--time-budget",
        "600",
        "-p",
        "custom",
        "-m",
        "deepseek-v4.1-flash-1017",
        "--out",
        str(out_json),
    ]
    with open(rdir / "bench.log", "w") as logf:
        subprocess.run(cmd, cwd="/tmp/hoplite/workspace", env=env, stdout=logf, stderr=subprocess.STDOUT)
    (rdir / "done.flag").write_text("1")
    srv.shutdown()


def start(run_id: str, task: str, variant: str, port: int):
    rdir = BASE_DIR / run_id
    if (rdir / "done.flag").exists():
        print("Run already completed; only one attempt per run_id is permitted.")
        _print_done(rdir / "results.json", rdir)
        return
    if (rdir / "turn_req.json").exists():
        req_data = json.loads((rdir / "turn_req.json").read_text())
        queue_and_print_page(rdir, run_id, render_turn_text(req_data))
        return
    rdir.mkdir(parents=True, exist_ok=True)
    TURNS_DIR.mkdir(parents=True, exist_ok=True)
    turn_py = TURNS_DIR / f"{run_id}.py"
    turn_py.write_text(
        'THOUGHT = ""\nCONTENT = ""\nCALLS = [\n    # {"name": "...", "arguments": {...}},\n]\n'
    )
    subprocess.Popen(
        [sys.executable, "/tmp/hoplite/workspace/scripts/ds_driver.py", "_daemon", run_id, task, variant, str(port)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    req_file = rdir / "turn_req.json"
    done_file = rdir / "done.flag"
    t0 = time.time()
    while not req_file.exists():
        if done_file.exists():
            print("Daemon exited early:", (rdir / "bench.log").read_text())
            return
        if time.time() - t0 > 60:
            raise TimeoutError("Timed out waiting for turn 1 request")
        time.sleep(0.05)
    req_data = json.loads(req_file.read_text())
    queue_and_print_page(rdir, run_id, render_turn_text(req_data))


def step(run_id: str):
    turn_py = TURNS_DIR / f"{run_id}.py"
    ns = {}
    exec(turn_py.read_text(), ns)
    calls = ns.get("CALLS") or []
    content = ns.get("CONTENT") or ""
    thought = ns.get("THOUGHT") or ""
    reply(run_id, calls=calls, content=content, thought=thought)


def reply(run_id: str, calls=None, content: str = "", thought: str = ""):
    rdir = BASE_DIR / run_id
    req_file = rdir / "turn_req.json"
    done_file = rdir / "done.flag"
    res_file = rdir / "results.json"
    if done_file.exists():
        _print_done(res_file, rdir)
        return
    cur_turn = json.loads(req_file.read_text())["turn"]
    payload = {"calls": calls or [], "content": content, "thought": thought}
    tmp = rdir / "turn_resp.json.tmp"
    tmp.write_text(json.dumps(payload))
    tmp.replace(rdir / "turn_resp.json")

    t0 = time.time()
    while True:
        if done_file.exists():
            _print_done(res_file, rdir)
            return
        if req_file.exists():
            try:
                data = json.loads(req_file.read_text())
                if data["turn"] > cur_turn:
                    queue_and_print_page(rdir, run_id, render_turn_text(data))
                    return
            except Exception:
                pass
        if time.time() - t0 > 180:
            raise TimeoutError("Timed out waiting for next turn or completion")
        time.sleep(0.05)


def _print_done(res_file: Path, rdir: Path):
    print("=== RUN COMPLETE ===")
    if res_file.exists():
        d = json.loads(res_file.read_text())
        for r in d.get("runs", []):
            st = r.get("stats", {})
            print(json.dumps({
                "task": r.get("task"),
                "variant": r.get("variant"),
                "solved": r.get("solved"),
                "outcome": r.get("outcome"),
                "turns": st.get("turns"),
                "steps": st.get("steps"),
                "prompt_tokens": st.get("prompt_tokens"),
                "cached_tokens": st.get("cached_tokens"),
                "output_tokens": st.get("output_tokens"),
                "reasoning_tokens": st.get("reasoning_tokens"),
                "cost": round(st.get("cost", 0.0), 5),
                "elapsed_ms": st.get("elapsed_ms"),
                "verify_tail": r.get("verify_tail", ""),
            }, indent=2))
    else:
        print((rdir / "bench.log").read_text())


if __name__ == "__main__":
    if len(sys.argv) >= 6 and sys.argv[1] == "_daemon":
        run_daemon(sys.argv[2], sys.argv[3], sys.argv[4], int(sys.argv[5]))
    elif len(sys.argv) >= 6 and sys.argv[1] == "start":
        start(sys.argv[2], sys.argv[3], sys.argv[4], int(sys.argv[5]))
    elif len(sys.argv) >= 3 and sys.argv[1] == "more":
        more(sys.argv[2])
    elif len(sys.argv) >= 3 and sys.argv[1] == "step":
        step(sys.argv[2])
