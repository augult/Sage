import json
import subprocess
import sys

seq, cmdfile = sys.argv[1], sys.argv[2]
command = open(cmdfile).read()
payload = {"calls": [{"name": "bash", "arguments": {"command": command}}]}
if len(sys.argv) > 3:
    payload["calls"][0]["arguments"]["workdir"] = sys.argv[3]
subprocess.run(
    [sys.executable, "/tmp/sage-hopliteloop.fazhjc/client.py", "ds-oc-pgb-py-bowling-1", seq, "reply"],
    input=json.dumps(payload),
    text=True,
)
