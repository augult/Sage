"""Offline checks for CLI exit status and the settings actually saved by interactive chat."""
import http.server
import json
import os
from pathlib import Path
import pty
import subprocess
import sys
import tempfile
import threading
import time


class LocalModel(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        self.rfile.read(int(self.headers["Content-Length"]))
        message = {"role": "assistant", "content": "Which filename should I use?"}
        body = json.dumps({"choices": [{"message": message, "finish_reason": "stop"}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def chat(binary, args, env, work, home, previous_turn):
    master, slave = pty.openpty()
    try:
        proc = subprocess.Popen([binary, "chat", *args, "Which file?"], cwd=work, env=env,
                                stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
        try:
            deadline = time.monotonic() + 25
            while time.monotonic() < deadline:
                for path in home.glob("sessions/*/snapshot.json"):
                    snapshot = json.loads(path.read_bytes())
                    if snapshot["turn"] > previous_turn and not snapshot["in_run"]:
                        return path.parent.name, snapshot
                if proc.poll() is not None:
                    raise RuntimeError(f"chat exited early with {proc.returncode}")
                time.sleep(0.05)
            raise RuntimeError("chat did not save a finished turn")
        finally:
            proc.terminate()
            proc.wait(timeout=5)
    finally:
        os.close(slave)
        os.close(master)


def main():
    binary = str(Path(sys.argv[1]).resolve())
    with http.server.ThreadingHTTPServer(("127.0.0.1", 0), LocalModel) as server:
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                work = root / "work"
                work.mkdir()
                state = root / "state"
                state.mkdir()
                env = {**os.environ, "SAGE_HOME": str(state), "NO_COLOR": "1",
                       "NO_PROXY": "127.0.0.1", "no_proxy": "127.0.0.1", "TERM": "xterm"}
                endpoint = {"base_url": f"http://127.0.0.1:{server.server_port}/v1", "models": ["fixture"]}
                (state / "config.json").write_text(json.dumps({"provider": "custom", "custom": endpoint}))

                result = subprocess.run([binary, "run", "--json", "--max-turns", "1", "Which file?"],
                                        cwd=work, env=env, capture_output=True, text=True, timeout=25)
                assert result.returncode == 2 and json.loads(result.stdout)["status"] == "yielded", result
                first = json.loads(result.stdout)["id"]
                saved = json.loads((state / "sessions" / first / "snapshot.json").read_bytes())
                assert saved["autonomy"] == "auto" and not saved["interactive"]
                session, resumed = chat(binary, ["--ask", "--resume", first, "--max-turns", "3",
                                                  "--time-budget", "19"], env, work, state, saved["turn"])
                assert session == first and resumed["autonomy"] == "ask" and resumed["interactive"]
                assert (resumed["max_turns"], resumed["time_budget_secs"]) == (3, 19)
                _, kept = chat(binary, ["--continue"], env, work, state, resumed["turn"])
                assert kept["autonomy"] == "ask" and (kept["max_turns"], kept["time_budget_secs"]) == (3, 19)

                fresh_state = root / "fresh"
                fresh_state.mkdir()
                (fresh_state / "config.json").write_bytes((state / "config.json").read_bytes())
                _, fresh = chat(binary, ["--max-turns", "1", "--time-budget", "15"],
                                {**env, "SAGE_HOME": str(fresh_state)}, work, fresh_state, 0)
                assert (fresh["max_turns"], fresh["time_budget_secs"]) == (1, 15)
                print("Verified fresh chat, resumed permissions and budgets, and unattended handoff exit.")
        finally:
            server.shutdown()


if __name__ == "__main__":
    main()
