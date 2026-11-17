#!/usr/bin/env bash
# Unharnessed (one tool: a persistent Python REPL): headless print mode, JSON event stream.
# Built from source into /opt/unharnessed; the kernel venv is prebuilt there and shared read-only.
. "$(dirname "$0")/../common.sh"
UH=/opt/unharnessed/unharnessed
need "$UH"

export UNHARNESSED_KERNEL_VENV=/opt/unharnessed/venv UNHARNESSED_KERNEL_PYTHON=/opt/unharnessed/venv/bin/python UNHARNESSED_OFFLINE=1
mkdir -p "$HOME/.unharnessed"
cat >"$HOME/.unharnessed/models.json" <<JSON
{
  "providers": {
    "bench": {
      "baseUrl": "$BENCH_BASE_URL",
      "api": "openai-completions",
      "apiKey": "$BENCH_API_KEY",
      "models": [{ "id": "$BENCH_MODEL" }]
    }
  }
}
JSON

set -o pipefail
# drop per-token message_update events: they repeat the whole message and made logs reach gigabytes
"$UH" --mode json --no-session --provider bench --model "$BENCH_MODEL" -p "$BENCH_PROMPT" | grep --line-buffered -v '^{"type":"message_update"'
