"""Housekeeping for the folder the nightly reports are written to."""

import argparse
from pathlib import Path


def rotate(folder, keep=7):
    """Deletes all but the newest `keep` report files (by name); returns the names deleted."""
    reports = sorted(Path(folder).glob("report-*.txt"))
    stale = reports[:-keep] if keep > 0 else reports
    for path in stale:
        path.unlink()
    return [path.name for path in stale]


def vacuum_all(folder):
    removed = 0
    for path in Path(folder).rglob("*.tmp"):
        path.unlink()
        removed += 1
    return removed


def prune_main(argv=None):
    """`stockr-prune FOLDER [--keep N]`: rotate the reports in a folder."""
    parser = argparse.ArgumentParser(prog="stockr-prune")
    parser.add_argument("folder")
    parser.add_argument("--keep", type=int, default=7)
    args = parser.parse_args(argv)
    for name in rotate(args.folder, args.keep):
        print(f"deleted {name}")
    return 0
