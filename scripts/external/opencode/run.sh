#!/usr/bin/env bash
# opencode: `opencode run` with its default `build` agent, permissions auto-approved.
. "$(dirname "$0")/../common.sh"
OPENCODE="$EXT_HOME/opencode/node_modules/.bin/opencode"
need "$OPENCODE"

# The endpoint is declared as a provider of its own; the config lives outside the workspace.
export OPENCODE_CONFIG="$BENCH_SCRATCH/opencode.json"
cat >"$OPENCODE_CONFIG" <<EOF
{
  "\$schema": "https://opencode.ai/config.json",
  "autoupdate": false,
  "share": "disabled",
  "model": "bench/$BENCH_MODEL",
  "provider": {
    "bench": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "bench",
      "options": { "baseURL": "{env:BENCH_BASE_URL}", "apiKey": "{env:BENCH_API_KEY}" },
      "models": { "$BENCH_MODEL": { "name": "$BENCH_MODEL" } }
    }
  },
  "agent": { "build": { "steps": $BENCH_MAX_TURNS } }
}
EOF

exec "$OPENCODE" run --auto --model "bench/$BENCH_MODEL" "$BENCH_PROMPT"
