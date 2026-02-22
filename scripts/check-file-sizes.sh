#!/usr/bin/env bash
set -euo pipefail

SOFT_LIMIT=500
HARD_LIMIT=700
exit_code=0

while IFS= read -r file; do
    lines=$(wc -l < "$file")
    if [ "$lines" -gt "$HARD_LIMIT" ]; then
        echo "ERROR: $file has $lines lines (hard limit: $HARD_LIMIT)"
        exit_code=1
    elif [ "$lines" -gt "$SOFT_LIMIT" ]; then
        echo "WARNING: $file has $lines lines (soft limit: $SOFT_LIMIT)"
    fi
done < <(find crates/*/src -name '*.rs' -type f)

if [ "$exit_code" -eq 0 ]; then
    echo "File size check passed."
fi

exit $exit_code
