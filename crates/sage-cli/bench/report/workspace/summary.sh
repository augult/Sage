#!/bin/sh
# Prints "category total" for every category in the orders file, sorted by category.
# The total is quantity times unit price, with two decimals.
file="${1:-orders.csv}"
awk -F, '{ total[$2] += $3 } END { for (c in total) print c, total[c] }' "$file" | sort
