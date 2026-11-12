#!/usr/bin/env bash
# Installs the external harnesses the benchmark can run, at pinned versions, outside the repository.
#   scripts/external/install.sh [name ...]     default: all of them
# Needs uv (Python harnesses), npm (Node harnesses) and curl. Safe to run again.
set -euo pipefail

EXT_HOME="${BENCH_EXT_HOME:-$HOME/.local/share/sage-external}"

MINI_VERSION=2.4.6
AIDER_VERSION=0.86.2
OPENCODE_VERSION=1.19.5
QWEN_VERSION=0.27.2
CODEX_VERSION=0.167.1
PI_VERSION=1.5.2
GOOSE_VERSION=1.59.0

# True when `$EXT_HOME/<name>` already holds this version.
have() {
    [ "$(cat "$EXT_HOME/$1/.version" 2>/dev/null)" = "$2" ]
}

mark() {
    echo "$2" >"$EXT_HOME/$1/.version"
    echo "installed $1 $2"
}

python_tool() { # name package version
    have "$1" "$3" && return 0
    rm -rf "${EXT_HOME:?}/$1"
    uv venv --quiet --python 3.12 "$EXT_HOME/$1"
    uv pip install --quiet --python "$EXT_HOME/$1/bin/python" "$2==$3"
    mark "$1" "$3"
}

node_tool() { # name package version
    have "$1" "$3" && return 0
    rm -rf "${EXT_HOME:?}/$1"
    mkdir -p "$EXT_HOME/$1"
    npm install --silent --no-fund --no-audit --prefix "$EXT_HOME/$1" "$2@$3"
    mark "$1" "$3"
}

goose_tool() {
    have goose "$GOOSE_VERSION" && return 0
    local arch
    case "$(uname -sm)" in
        "Linux x86_64") arch=x86_64-unknown-linux-gnu ;;
        "Linux aarch64") arch=aarch64-unknown-linux-gnu ;;
        "Darwin arm64") arch=aarch64-apple-darwin ;;
        "Darwin x86_64") arch=x86_64-apple-darwin ;;
        *) echo "goose: no release for $(uname -sm)" >&2; return 1 ;;
    esac
    rm -rf "${EXT_HOME:?}/goose"
    mkdir -p "$EXT_HOME/goose"
    curl -fsSL "https://github.com/block/goose/releases/download/v$GOOSE_VERSION/goose-$arch.tar.bz2" | tar -xj -C "$EXT_HOME/goose"
    # The archive holds the binary either at its top or in one directory.
    [ -x "$EXT_HOME/goose/goose" ] || mv "$EXT_HOME"/goose/*/goose "$EXT_HOME/goose/goose"
    mark goose "$GOOSE_VERSION"
}

install() {
    case "$1" in
        mini) python_tool mini mini-swe-agent "$MINI_VERSION" ;;
        aider) python_tool aider aider-chat "$AIDER_VERSION" ;;
        opencode) node_tool opencode opencode-ai "$OPENCODE_VERSION" ;;
        qwen) node_tool qwen @qwen-code/qwen-code "$QWEN_VERSION" ;;
        codex) node_tool codex @openai/codex "$CODEX_VERSION" ;;
        pi) node_tool pi @earendil-works/pi-coding-agent "$PI_VERSION" ;;
        goose) goose_tool ;;
        *) echo "unknown harness $1: mini, opencode, aider, qwen, goose, codex or pi" >&2; return 2 ;;
    esac
}

mkdir -p "$EXT_HOME"
[ $# -gt 0 ] || set -- mini opencode aider qwen goose codex pi
failed=()
for name in "$@"; do
    install "$name" || failed+=("$name")
done
if [ ${#failed[@]} -gt 0 ]; then
    echo "not installed: ${failed[*]}" >&2
    exit 1
fi
