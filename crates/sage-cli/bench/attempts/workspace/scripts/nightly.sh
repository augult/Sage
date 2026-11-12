#!/bin/sh
# Nightly refresh of the mirror. The upstream is flaky at night, so every download gets four
# retries instead of the default two. Extra arguments are passed on to fetchkit.
set -eu
cd "$(dirname "$0")/.."
exec python3 -m fetchkit fetch --retries 4 --manifest examples/manifest.json "$@"
