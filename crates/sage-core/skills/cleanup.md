---
name: cleanup
title: Clean up
description: Remove dead code, dead files and noise without changing behaviour.
---
Remove what is not used and change nothing that is. Behaviour, public API and output stay identical.

## Before deleting anything
1. Take a baseline: run the tests, build and lint, and note the result. With git, look at `git status` first so your changes stay separable.
2. Find the roots. Everything reachable from them is live:
   - entry points: `main`, console scripts, `bin/`, `package.json` scripts and exports, `pyproject` entry points, Makefile, CI files, Dockerfile
   - public API: `__all__`, index and export files, names the documentation shows
   - tests, and configuration that names code: plugin lists, routes, settings strings, templates

## What counts as dead
A symbol or file is dead only if a search of the whole repository for its name finds nothing but its definition, and nothing reaches it dynamically. Search the name as plain text, everywhere: code, tests, scripts, configuration, documentation, templates. An import search is not enough. Dynamic use hides in:
- `getattr`, `importlib`, `globals()`, registries and dispatch tables, decorators that register
- names built from strings (`"handle_" + kind`), class names stored in data
- framework conventions: test and fixture discovery, migrations, signal handlers, magic methods, commands found by name
- re-exports

Code used only by its own tests is dead only if it is not public API; then remove both. When you cannot prove it dead, keep it and list it in the summary.

## Remove
- Unused functions, classes, methods, private parameters, variables, constants, imports and dependencies; unreachable branches; files nothing references; flags that are always off.
- Commented-out code. Comments that restate the code, are out of date, or are noise (banners, "added by", change logs). Leftover debug output.
- Keep comments that say why, warn of a pitfall or document public API, and keep licence headers.

## Not part of this job
No renaming, reformatting, reordering, restructuring or modernising. No changes to public signatures. Bugs you notice are reported, not fixed. The diff should be almost purely deletions.

## How
- Work in small batches (one module, or one kind of thing) and re-run the checks after each. If something breaks, it was not dead: put it back.
- After each removal, check what only the removed thing was using: imports, helpers, fixtures, files, dependencies. That is dead now too. Repeat until nothing more falls out.
- At the end search once more for every removed name: no import, export, document or configuration entry may still point at it.

## Finish
The checks pass exactly as in the baseline. The summary lists what was removed (grouped), what looked dead but was kept and why, and anything suspicious you left alone.
