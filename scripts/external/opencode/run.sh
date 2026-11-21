#!/usr/bin/env bash
# opencode: `opencode run` with its default `build` agent, permissions auto-approved.
. "$(dirname "$0")/../common.sh"
OPENCODE="$EXT_HOME/opencode/node_modules/.bin/opencode"
need "$OPENCODE"

# Fetches at startup that only describe the harness (the models catalog, the default plugins from
# npm): a benchmark that gives the agent no network refuses them, and opencode then hangs.
export OPENCODE_DISABLE_MODELS_FETCH=1 OPENCODE_DISABLE_DEFAULT_PLUGINS=1 OPENCODE_DISABLE_AUTOUPDATE=1 OPENCODE_DISABLE_LSP_DOWNLOAD=1

# The endpoint is declared as a provider of its own; the config lives outside the workspace.
# BENCH_EFFORT is the reasoning effort Sage's runs start at: asked for the same way.
options="{}"
case "${BENCH_EFFORT:-}" in
    "" | off) ;;
    *) options="{ \"reasoningEffort\": \"$BENCH_EFFORT\" }" ;;
esac
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
      "models": { "$BENCH_MODEL": { "name": "$BENCH_MODEL", "options": $options } }
    }
  },
  "agent": { "build": { "steps": $BENCH_MAX_TURNS } }
}
EOF

# The task is on the command line, where an agent's `pkill -f <a word of the task>` can hit this
# process, its own or a neighbouring run's. Passing it on standard input instead was not confirmed
# to work with this version.
exec "$OPENCODE" run --auto --model "bench/$BENCH_MODEL" "$BENCH_PROMPT"
