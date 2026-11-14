"""Turn exercises of the Aider polyglot benchmark into tasks for `sage bench --tasks-dir`.

usage: prepare.py <polyglot-benchmark checkout> <out dir> [python]

The benchmark (https://github.com/Aider-AI/polyglot-benchmark) is 225 Exercism exercises in six
languages. Each becomes a task whose workspace holds the stub to complete and `INSTRUCTIONS.md`
(the exercise's introduction, instructions and appendix, as Aider joins them). The exercise's
tests are held out and added only to verify, and its example solution is what `--check` proves
the task with. The agent gets one go and never sees a test: Aider's own protocol shows the test
output after a first failure and allows a second try, so figures are comparable with its
first-try rate, not with its headline.
"""
import json
import pathlib
import shutil
import sys

LANGS = {
    "python": {
        "needs": ["python3"],
        "check": lambda stubs: "python3 -m py_compile " + " ".join(stubs),
        "verify": lambda tests: "python3 -m unittest " + " ".join(t for t in tests if t.endswith("_test.py")),
        "library": "Use only the standard library.",
    },
}
PROMPT = "`INSTRUCTIONS.md` describes an exercise. Implement it in {stubs}. Do not change the names of the functions and classes that are already there: other code, such as unit tests you are not shown, refers to them. {library}"


def main():
    src, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
    langs = sys.argv[3:] or list(LANGS)
    made = skipped = 0
    for lang in langs:
        how = LANGS[lang]
        for ex in sorted((src / lang / "exercises" / "practice").iterdir()):
            cfg = json.loads((ex / ".meta" / "config.json").read_text())
            files = cfg["files"]
            stubs, tests, examples = files["solution"], files["test"], files["example"]
            if len(stubs) != len(examples):
                print(f"skipped {lang}/{ex.name}: {len(stubs)} stubs, {len(examples)} examples")
                skipped += 1
                continue
            task = out / f"{lang}-{ex.name}"
            shutil.rmtree(task, ignore_errors=True)
            held = set(tests) | set(examples)
            for path in sorted(p for p in ex.rglob("*") if p.is_file()):
                rel = path.relative_to(ex).as_posix()
                if rel.startswith((".docs/", ".meta/")) or rel in held:
                    continue
                (task / "workspace" / rel).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy(path, task / "workspace" / rel)
            parts = [ex / ".docs" / n for n in ("introduction.md", "instructions.md", "instructions.append.md")]
            (task / "workspace" / "INSTRUCTIONS.md").write_text("\n".join(p.read_text().rstrip() + "\n" for p in parts if p.exists()))
            for rel in tests:
                (task / "hidden" / rel).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy(ex / rel, task / "hidden" / rel)
            for stub, example in zip(stubs, examples):
                (task / "solution" / stub).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy(ex / example, task / "solution" / stub)
            (task / "task.json").write_text(
                json.dumps(
                    {
                        "title": cfg.get("blurb", ex.name),
                        "prompt": PROMPT.format(stubs=", ".join(f"`{s}`" for s in stubs), library=how["library"]),
                        "needs": how["needs"],
                        "check": how["check"](stubs),
                        "verify": how["verify"](tests),
                    },
                    indent=2,
                )
                + "\n"
            )
            made += 1
    print(f"{made} tasks in {out}" + (f", {skipped} skipped" if skipped else ""))


main()
