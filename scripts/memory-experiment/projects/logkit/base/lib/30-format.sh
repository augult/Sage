# cmd_fmt: "[LEVEL] component: message", without the timestamp.
cmd_fmt() {
  while IFS= read -r line; do
    rest=${line#* }
    level=${rest%% *}
    printf '[%s] %s\n' "$level" "${rest#* }"
  done
}
