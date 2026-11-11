#!/usr/bin/env bash
# Qwen Code: headless with a positional prompt, every action accepted (--yolo).
. "$(dirname "$0")/../common.sh"
QWEN="$EXT_HOME/qwen/node_modules/.bin/qwen"
need "$QWEN"

code=0
"$QWEN" --yolo --auth-type openai --openai-base-url "$BENCH_BASE_URL" --openai-api-key "$BENCH_API_KEY" \
    --model "$BENCH_MODEL" --max-session-turns "$BENCH_MAX_TURNS" --output-format json "$BENCH_PROMPT" || code=$?
# 53 is how Qwen Code says that the session's turns are used up.
[ "$code" -ne 53 ] || code=124
exit "$code"
