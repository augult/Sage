#!/bin/sh
# Prints level_num() and LEVELS_DESC (names, most severe first) from the level table: NAME NUMBER per line.
echo 'level_num() {'
echo '  case "$1" in'
awk 'NF == 2 { printf "    %s) echo %s ;;\n", $1, $2 }' "$1"
echo '    *) echo 0 ;;'
echo '  esac'
echo '}'
printf 'LEVELS_DESC="%s"\n' "$(sort -k2,2nr "$1" | awk 'NF == 2 { printf "%s%s", sep, $1; sep = " " }')"
