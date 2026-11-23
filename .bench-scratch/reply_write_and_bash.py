import json
import subprocess
import sys

seq, source, target, cmdfile, workdir = sys.argv[1:6]
payload = {
    "calls": [
        {"name": "write", "arguments": {"filePath": target, "content": open(source).read()}},
        {"name": "bash", "arguments": {"command": open(cmdfile).read(), "workdir": workdir}},
    ]
}
subprocess.run(
    [sys.executable, "/tmp/sage-hopliteloop.fazhjc/client.py", "ds-oc-pgb-py-bowling-1", seq, "reply"],
    input=json.dumps(payload),
    text=True,
)
