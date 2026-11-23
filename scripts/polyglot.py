"""Prepare a separate Sage checkout whose `sage bench` has the 23 Exercism exercises of the
benchmark in docs/BENCHMARKS.md as the task set `polyblind`, their tests held out.

    python3 scripts/polyglot.py <new directory>
    cd <new directory> && cargo run -p sage-cli -- bench --tasks polyblind ...
"""
import argparse
import csv
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

SAGE = Path(__file__).resolve().parents[1]
# The exercises are the ones in the published runs.
RUNS = SAGE / "docs/results/2026-11-22-deepseek-v4.1-flash/runs.csv"
UPSTREAM = "https://github.com/Aider-AI/polyglot-benchmark.git"
REVISION = "7e0611e77b54e2dea774cdc0aa00cf9f7ed6144f"
# Language, tool available to the agent, check before grading, held-out grader.
LANGUAGES = {
    "py": ("python", "python3", "python3 -m compileall -q .",
           "python3 -m unittest discover -p '*_test.py'"),
    "rs": ("rust", "cargo", "cargo check --offline",
           "cargo test --offline --all-features -- --include-ignored"),
    "go": ("go", "go", "go build ./...", "go test -count=1 ./..."),
}


def git(*args, cwd=None):
    return subprocess.check_output(["git", *map(str, args)], cwd=cwd, text=True).strip()


def exercise(upstream, bench, task):
    _, short, slug = task.split("-", 2)
    lang, tool, check, verify = LANGUAGES[short]
    source = upstream / lang / "exercises" / "practice" / slug
    meta = json.loads((source / ".meta/config.json").read_text(encoding="utf-8"))
    paths = meta["files"]
    tests = set(paths["test"])
    if short == "go":
        tests.update(paths.get("editor", []))
    starter = set(paths["solution"] + paths.get("editor", []) + paths.get("invalidator", [])) - tests
    implementation = next(name for name in paths["solution"] if name != "Cargo.toml")
    example, = paths["example"]
    folder = bench / task
    folder.mkdir()
    instructions = [(source / ".docs/instructions.md").read_text(encoding="utf-8")]
    appendix = source / ".docs/instructions.append.md"
    if appendix.is_file():
        instructions.append(appendix.read_text(encoding="utf-8"))

    def copy(group, dest, original):
        target = folder / group / dest
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / original, target)

    for name in sorted(starter):
        copy("workspace", name, name)
    for name in sorted(tests):
        copy("hidden", name, name)
    copy("solution", implementation, example)
    (folder / "workspace/README.md").write_text("\n\n".join(instructions), encoding="utf-8")
    starter.add("README.md")

    def quoted(names):
        return ", ".join(json.dumps(name) for name in sorted(names))

    def embedded(group, names):
        return f'embed!("{task}/{group}": {quoted(names)})'

    return f'''    Task {{
        id: "{task}",
        title: "{lang}: {slug}",
        prompt: "Implement the exercise described in README.md. The grader will supply its tests after you finish.",
        needs: &["{tool}"],
        files: {embedded("workspace", starter)},
        check: "{check}",
        restore: &[{quoted(starter - {implementation})}],
        hidden: {embedded("hidden", tests)},
        verify: "{verify}",
        solution: {embedded("solution", [implementation])},
        skill: "",
    }},'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new directory for the benchmark checkout")
    args = parser.parse_args()
    output = args.output.resolve()
    if output.exists() or SAGE in output.parents:
        parser.error("choose a new directory outside the Sage checkout")
    with RUNS.open(newline="", encoding="utf-8") as rows:
        tasks = sorted({row["task"] for row in csv.DictReader(rows) if row["task"].startswith("pgb-")})
    if len(tasks) != 23:
        raise ValueError(f"expected 23 exercise IDs in {RUNS}, found {len(tasks)}")

    git("clone", "--quiet", "--no-checkout", "--no-hardlinks", SAGE, output)
    git("checkout", "--quiet", "--detach", git("rev-parse", "HEAD", cwd=SAGE), cwd=output)
    cli = output / "crates/sage-cli"
    with tempfile.TemporaryDirectory() as temp:
        git("init", "--quiet", temp)
        git("fetch", "--quiet", "--depth=1", UPSTREAM, REVISION, cwd=temp)
        git("checkout", "--quiet", "--detach", REVISION, cwd=temp)
        definitions = [exercise(Path(temp), cli / "bench", task) for task in tasks]
    registry = cli / "src/bench/tasks.rs"
    code = registry.read_text(encoding="utf-8")
    marker = '    ("easy", TASKS),'
    if code.count(marker) != 1:
        raise ValueError("the task registry has changed; could not register polyblind")
    code = code.replace("pub mod hard;", "pub mod hard;\nmod polyblind;")
    code = code.replace(marker, marker + '\n    ("polyblind", polyblind::TASKS),')
    registry.write_text(code, encoding="utf-8")
    (registry.parent / "tasks/polyblind.rs").write_text(
        "use super::Task;\n\npub const TASKS: &[Task] = &[\n" + "\n".join(definitions) + "\n];\n",
        encoding="utf-8",
    )
    print(f"Prepared {len(tasks)} tasks in {output}")


if __name__ == "__main__":
    main()
