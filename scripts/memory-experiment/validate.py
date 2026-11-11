#!/usr/bin/env python3
"""Offline check of the memory-experiment task set (no model, no network).

For every task of every project: the friction is present at the starting state, the visible tests
pass there with the workaround unless the task is a bug with a failing test, the hidden check fails
at the start and passes on the solved project.

    python3 scripts/memory-experiment/validate.py
"""
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile

PROJECTS = pathlib.Path(__file__).resolve().parent / "projects"


def overlay(src: pathlib.Path, dest: pathlib.Path) -> None:
    if src.is_dir():
        shutil.copytree(src, dest, dirs_exist_ok=True)


def materialize(project: pathlib.Path, task: str, dest: pathlib.Path, solved: bool = False) -> None:
    """The task's starting state (or the solved project) in `dest`, which keeps its path."""
    dest.mkdir(parents=True, exist_ok=True)
    for child in dest.iterdir():
        shutil.rmtree(child) if child.is_dir() else child.unlink()
    overlay(project / "base", dest)
    if not solved:
        overlay(project / "tasks" / task / "start", dest)


def sh(cmd: str, cwd: pathlib.Path) -> tuple[bool, str]:
    try:
        r = subprocess.run(["sh", "-c", cmd], cwd=cwd, capture_output=True, text=True, timeout=120)
    except subprocess.TimeoutExpired:
        return False, "timed out"
    return r.returncode == 0, r.stdout + r.stderr


def hidden_check(project: pathlib.Path, meta: dict, task: str, ws: pathlib.Path) -> tuple[bool, str]:
    """Runs the held-out check on a copy of `ws`, with the tests restored to their original content."""
    with tempfile.TemporaryDirectory(prefix="memexp-check-") as tmp:
        copy = pathlib.Path(tmp) / meta["name"]
        shutil.copytree(ws, copy)
        pristine = pathlib.Path(tmp) / "pristine"
        materialize(project, task, pristine)
        for rel in meta["pristine"]:
            shutil.rmtree(copy / rel, ignore_errors=True)
            shutil.copytree(pristine / rel, copy / rel)
        overlay(project / "tasks" / task / "hidden", copy)
        return sh(meta["check"], copy)


def main() -> int:
    bad = 0
    for project in sorted(p for p in PROJECTS.iterdir() if (p / "project.json").exists()):
        meta = json.loads((project / "project.json").read_text())
        for task in meta["tasks"]:
            with tempfile.TemporaryDirectory(prefix="memexp-validate-") as tmp:
                ws = pathlib.Path(tmp) / meta["name"]
                materialize(project, task["id"], ws)
                plain_ok, plain_out = sh(meta["plain"], ws)
                hidden_start, _ = hidden_check(project, meta, task["id"], ws)
                visible_ok, _ = sh(meta["visible"], ws)
                materialize(project, task["id"], ws, solved=True)
                hidden_solved, solved_out = hidden_check(project, meta, task["id"], ws)
            checks = {
                "friction at start": not plain_ok and any(m in plain_out for m in meta["friction_markers"]),
                "visible tests with workaround": visible_ok == (task["kind"] != "bugfix-failing-test"),
                "hidden check fails at start": not hidden_start,
                "hidden check passes when solved": hidden_solved,
            }
            failed = [name for name, ok in checks.items() if not ok]
            bad += len(failed)
            print(f"{meta['name']:9} {task['id']} {task['kind']:20} {'ok' if not failed else 'BAD: ' + ', '.join(failed)}")
            if "hidden check passes when solved" in failed:
                print(solved_out[-600:])
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
