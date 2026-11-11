# cmd_filter [--min LEVEL] [--component NAME]: lines at or above LEVEL, optionally from one component only.
cmd_filter() {
  min=0
  component=
  while [ $# -gt 0 ]; do
    case "$1" in
      --min)
        min=$(level_num "$2")
        [ "$min" -gt 0 ] || die "unknown level: $2"
        shift 2 ;;
      --component)
        component=$2
        shift 2 ;;
      *) die "filter: unknown option $1" ;;
    esac
  done
  while IFS= read -r line; do
    set -f; set -- $line; set +f
    [ "$(level_num "$2")" -ge "$min" ] || continue
    [ -z "$component" ] || [ "${3%:}" = "$component" ] || continue
    printf '%s\n' "$line"
  done
}

# cmd_since TIMESTAMP: lines stamped at or after TIMESTAMP (ISO 8601 UTC, so plain string order).
cmd_since() {
  [ $# -eq 1 ] || die "usage: logkit since TIMESTAMP"
  awk -v since="$1" '($1 "") >= since'
}
