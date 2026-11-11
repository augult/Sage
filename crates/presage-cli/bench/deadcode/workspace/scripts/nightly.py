"""Run from cron: writes today's report next to the earlier ones and rotates them.

    python3 scripts/nightly.py INVENTORY FOLDER
"""

import sys
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from stockr import Inventory, build_report  # noqa: E402
from stockr.maintenance import rotate  # noqa: E402


def main(argv):
    inventory_path, folder = argv[1], Path(argv[2])
    folder.mkdir(parents=True, exist_ok=True)
    report = folder / f"report-{date.today().isoformat()}.txt"
    report.write_text(build_report(Inventory.load(inventory_path)), encoding="utf-8")
    rotate(folder)
    print(report)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
