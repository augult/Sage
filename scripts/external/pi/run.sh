#!/usr/bin/env bash
# pi coding agent: print mode. It asks for no approvals and has no step limit of its own; the
# runner's turn limit applies.
. "$(dirname "$0")/../common.sh"
PI="$EXT_HOME/pi/node_modules/.bin/pi"
need "$PI"

export PI_CODING_AGENT_DIR="$BENCH_SCRATCH/pi"
mkdir -p "$PI_CODING_AGENT_DIR"
cat >"$PI_CODING_AGENT_DIR/models.json" <<EOF
{
  "providers": {
    "bench": {
      "baseUrl": "$BENCH_BASE_URL",
      "api": "openai-completions",
      "apiKey": "\$BENCH_API_KEY",
      "models": [{ "id": "$BENCH_MODEL" }]
    }
  }
}
EOF

exec "$PI" --print --provider bench --model "$BENCH_MODEL" "$BENCH_PROMPT"
