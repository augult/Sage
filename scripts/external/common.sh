# Sourced by every adapter. The runner provides BENCH_PROMPT, BENCH_MODEL, BENCH_BASE_URL,
# BENCH_API_KEY, BENCH_MAX_TURNS, BENCH_TIME_BUDGET and BENCH_SCRATCH; the cwd is the workspace.
# Exit 0 when the harness finished by itself, 124 when it ran into its own step limit.
set -euo pipefail

: "${BENCH_PROMPT:?}" "${BENCH_MODEL:?}" "${BENCH_BASE_URL:?}" "${BENCH_API_KEY:?}" "${BENCH_SCRATCH:?}"
: "${BENCH_MAX_TURNS:=30}" "${BENCH_TIME_BUDGET:=600}"

EXT_HOME="${BENCH_EXT_HOME:-$HOME/.local/share/sage-external}"

# A home of its own for the harness: no config, history or cache of the user's, none left behind.
export HOME="$BENCH_SCRATCH/home"
export XDG_CONFIG_HOME="$HOME/.config" XDG_DATA_HOME="$HOME/.local/share" XDG_STATE_HOME="$HOME/.local/state" XDG_CACHE_HOME="$HOME/.cache"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_CACHE_HOME"
export PYTHONDONTWRITEBYTECODE=1 NO_COLOR=1 CI=1 TERM=dumb

need() {
    [ -x "$1" ] || { echo "bench adapter: $1 is not installed; run scripts/external/install.sh" >&2; exit 127; }
}
