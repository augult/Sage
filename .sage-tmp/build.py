import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent
OUT = Path("/tmp/ds-sage-pgb-py-bowling-2-payloads")
OUT.mkdir(parents=True, exist_ok=True)

impl = (ROOT / "impl.py").read_text()
check = (ROOT / "check.py").read_text()

summary = (
    "Implemented BowlingGame in bowling.py: incremental rule validation "
    "(pins 0-10, frame totals, tenth-frame fill balls, no rolls past ten frames) "
    "and ten-frame scoring with strike/spare bonuses. Rule violations raise "
    "BowlingError(ValueError, IndexError) with a message, and a rejected roll "
    "leaves the game unchanged. Verified with a local check script covering "
    "known scores (0, 90, 150, 190, 299, 300, plus the README example) and every "
    "error case; the grader's own tests were not available."
)

payload = {
    "calls": [
        {"name": "write", "arguments": {"path": "bowling.py", "content": impl}},
        {
            "name": "write",
            "arguments": {"path": "check_bowling_rules.py", "content": check},
        },
        {
            "name": "shell",
            "arguments": {
                "cmd": "python3 check_bowling_rules.py",
                "expect_exit": "0",
                "expect_has": "ALL OK",
            },
        },
        {"name": "finish", "arguments": {"summary": summary}},
    ]
}

(OUT / "reply1.json").write_text(json.dumps(payload))
print("wrote", OUT / "reply1.json")
