# Memory experiment

Material for measuring whether reflex memory (`lessons`) improves results. No live sessions have
been run: the task set and the mechanism replay below are checked offline only, and the driver that
runs and scores sessions is not written.

- `projects/<name>/` — three synthetic projects whose tests only run with a workaround
  (`ledger`: `PYTHONPATH=src`; `logkit`: `make gen` first; `shopcart`: a Node test-runner flag).
  `base/` is the solved project. `tasks/tN/start/` is laid over it for a task's starting state,
  `tasks/tN/hidden/` holds the held-out check. `project.json` has the task texts, the check command
  and the three-line note for a NOTE arm.
- `validate.py` — checks every task without a model: the friction is there at the start, the hidden
  check fails at the start and passes on `base/`.
- `replay/` — feeds the command sequences these projects produce to the real `LessonBook` and prints
  what it learns and hints. Output: `docs/results/memory/lessons-replay.txt`.

```
python3 scripts/memory-experiment/validate.py
cargo run -q --manifest-path scripts/memory-experiment/replay/Cargo.toml
```
