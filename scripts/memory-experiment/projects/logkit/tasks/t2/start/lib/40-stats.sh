# cmd_stats: number of lines per level, most severe first; levels that do not occur are left out.
cmd_stats() {
  awk '{ n[$2]++ } END { for (level in n) print level, n[level] }' | sort
}
