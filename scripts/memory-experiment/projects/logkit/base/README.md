# logkit

Shell helpers for reading service logs (`TIMESTAMP LEVEL COMPONENT: MESSAGE`).

    logkit filter --min WARN < service.log

- `bin/logkit` — entry point
- `lib/` — the commands, one file per area
- `data/levels.txt` — the level table
- `tests/run.sh` — test runner; cases live in `tests/cases/`
