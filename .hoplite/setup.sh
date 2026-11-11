#!/usr/bin/env bash
# Hoplite workspace setup. The platform provisions Rust from rust-toolchain.toml; this adds the
# system libraries the Tauri shell links against and the tools for the headless screenshots in
# docs/DESKTOP_UI_BRIEF.md. The package list lives in scripts/setup-dev.sh.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
exec bash scripts/setup-dev.sh --system-only
