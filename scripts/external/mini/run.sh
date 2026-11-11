#!/usr/bin/env bash
# mini-swe-agent 2.x: its default `mini.yaml` (one bash tool, tool calling), unattended.
. "$(dirname "$0")/../common.sh"
MINI="$EXT_HOME/mini/bin/mini"
need "$MINI"

export MSWEA_CONFIGURED=true MSWEA_SILENT_STARTUP=1 MSWEA_GLOBAL_CONFIG_DIR="$BENCH_SCRATCH/config"
# litellm has no price for the model; mini's own cost accounting is not used.
export MSWEA_COST_TRACKING=ignore_errors
export OPENAI_API_KEY="$BENCH_API_KEY"

# The task goes in through the environment, not the command line: an agent that runs
# `pkill -f <a word of the task>` must not hit the harness, its own or a neighbouring run's.
"$EXT_HOME/mini/bin/python" -c 'import os, sys
from minisweagent.run.mini import app
sys.argv = ["mini", "--task", os.environ["BENCH_PROMPT"]] + sys.argv[1:]
sys.exit(app())' --model "openai/$BENCH_MODEL" --yolo --exit-immediately --cost-limit 0 \
    -c mini.yaml -c "agent.step_limit=$BENCH_MAX_TURNS" -c "model.model_kwargs.api_base=$BENCH_BASE_URL" \
    --output "$BENCH_SCRATCH/trajectory.json"

# mini exits 0 whatever stopped it; its trajectory says what did.
if grep -Eq '"exit_status": *"LimitsExceeded"' "$BENCH_SCRATCH/trajectory.json"; then
    exit 124
fi
