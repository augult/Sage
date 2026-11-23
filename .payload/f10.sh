#!/bin/sh
# Nightly refresh of the mirror. The upstream is flaky at night, so every download gets five
# tries instead of the default three. Extra arguments are passed on to fetchkit.
set -eu
cd "$(dirname "$0")/.."
exec python3 -m fetchkit fetch --attempts 5 --manifest examples/manifest.json "$@"
