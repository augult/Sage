#!/bin/sh
# Prints "category total" for every category in the orders file, sorted by category.
# The total is quantity times unit price, with two decimals.
file="${1:-orders.csv}"
awk -F, 'NR > 1 { total[$2] += $3 * $4 } END { for (c in total) printf "%s %.2f\n", c, total[c] }' "$file" | sort
