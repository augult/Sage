# cmd_fmt: "[LEVEL] component: message", without the timestamp.
cmd_fmt() {
  awk '{
    level = $2
    sub(/^[^ ]+ [^ ]+ /, "")
    split($0, part, ": ")
    printf "[%s] %s: %s\n", level, part[1], part[2]
  }'
}
