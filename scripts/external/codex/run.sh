#!/usr/bin/env bash
# OpenAI Codex CLI: `codex exec`, approvals and sandbox off. Codex speaks only the Responses API,
# so this works only where the provider serves `/responses`, not on a plain chat-completions
# endpoint. It has no step limit of its own; the runner's turn limit applies.
. "$(dirname "$0")/../common.sh"
CODEX="$EXT_HOME/codex/node_modules/.bin/codex"
need "$CODEX"

export CODEX_HOME="$BENCH_SCRATCH/codex"
mkdir -p "$CODEX_HOME"

exec "$CODEX" exec --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox --ephemeral --color never \
    --model "$BENCH_MODEL" -c 'model_provider="bench"' \
    -c "model_providers.bench={ name = \"bench\", base_url = \"$BENCH_BASE_URL\", env_key = \"BENCH_API_KEY\", wire_api = \"responses\" }" \
    "$BENCH_PROMPT"
