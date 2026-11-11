#!/usr/bin/env bash
# aider: one `--message`, every confirmation answered yes. It edits files; unattended it never
# runs a command (its shell suggestions need an explicit yes), and it stops after 3 reflections.
. "$(dirname "$0")/../common.sh"
AIDER="$EXT_HOME/aider/bin/aider"
need "$AIDER"

# aider works on a git repository (its repo map). The workspace becomes one for the run only.
git init -q .
git add -A
git -c user.name=bench -c user.email=bench@localhost commit -q -m "task" || true
trap 'rm -rf .git .aider*' EXIT

"$AIDER" --model "openai/$BENCH_MODEL" --openai-api-base "$BENCH_BASE_URL" --openai-api-key "$BENCH_API_KEY" \
    --message "$BENCH_PROMPT" --yes-always \
    --no-check-update --no-analytics --no-show-release-notes --no-show-model-warnings --no-gitignore \
    --no-pretty --no-fancy-input \
    --chat-history-file "$BENCH_SCRATCH/chat.md" --input-history-file "$BENCH_SCRATCH/input.history"
