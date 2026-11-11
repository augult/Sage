#!/bin/sh
# Compares the summary of orders.csv with the expected one.
if sh summary.sh | diff - expected.txt; then
  echo "PASS: the summary matches expected.txt"
else
  echo "FAIL: the summary differs from expected.txt (lines marked < are yours, > are expected)"
  exit 1
fi
