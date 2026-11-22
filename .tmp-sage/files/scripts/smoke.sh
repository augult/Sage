#!/bin/sh
# Smoke check run by the deploy job: is the index reachable right now? One attempt, no patience,
# so that a dead upstream fails the deploy in seconds.
set -eu
cd "$(dirname "$0")/.."
FETCHKIT_ATTEMPTS=1 FETCHKIT_TIMEOUT=3 exec python3 -m fetchkit fetch https://mirror.example/index.json
