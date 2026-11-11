#!/usr/bin/env bash
# Goose: `goose run` with its default `developer` extension, tools approved automatically.
. "$(dirname "$0")/../common.sh"
GOOSE="$EXT_HOME/goose/goose"
need "$GOOSE"

# Goose takes the endpoint as a host and a path.
rest="${BENCH_BASE_URL#*://}"
export OPENAI_HOST="${BENCH_BASE_URL%%://*}://${rest%%/*}"
export OPENAI_BASE_PATH="${rest#*/}/chat/completions"
export OPENAI_API_KEY="$BENCH_API_KEY"
export GOOSE_PROVIDER=openai GOOSE_MODEL="$BENCH_MODEL" GOOSE_MODE=auto GOOSE_DISABLE_KEYRING=1 GOOSE_TELEMETRY_ENABLED=false

exec "$GOOSE" run --no-session --max-turns "$BENCH_MAX_TURNS" --text "$BENCH_PROMPT"
