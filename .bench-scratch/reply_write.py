import json
import subprocess
import sys

seq, source, target = sys.argv[1], sys.argv[2], sys.argv[3]
payload = {"calls": [{"name": "write", "arguments": {"filePath": target, "content": open(source).read()}}]}
subprocess.run(
    [sys.executable, "/tmp/sage-hopliteloop.fazhjc/client.py", "ds-oc-pgb-py-bowling-1", seq, "reply"],
    input=json.dumps(payload),
    text=True,
)
