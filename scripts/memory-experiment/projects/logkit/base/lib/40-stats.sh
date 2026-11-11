# cmd_stats: number of lines per level, most severe first; levels that do not occur are left out.
cmd_stats() {
  counts=$(awk '{ n[$2]++ } END { for (level in n) print level, n[level] }')
  for level in $LEVELS_DESC; do
    printf '%s\n' "$counts" | awk -v level="$level" '$1 == level'
  done
}
