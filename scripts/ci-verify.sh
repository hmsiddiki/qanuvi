#!/usr/bin/env bash
# CI: runs `npm run verify`; when it fails, its failing lines (or its last lines) are posted as an annotation
# (::error::), which the run's summary page and the checks API show without opening the log.
set -uo pipefail
cd "$(dirname "$0")/.."

npm run verify 2>&1 | tee /tmp/qanuvi-verify.txt
code=${PIPESTATUS[0]}
if [ "$code" -ne 0 ]; then
  # Rust and vitest failures first; otherwise the end of the output.
  lines=$(grep -E "FAILED|panicked|failures:|FAIL |AssertionError|^error|ERR!|Error:" /tmp/qanuvi-verify.txt | tail -n 40)
  [ -n "$lines" ] || lines=$(tail -n 60 /tmp/qanuvi-verify.txt)
  body=$(printf '%s\n--- end of output ---\n%s' "$lines" "$(tail -n 20 /tmp/qanuvi-verify.txt)" | sed 's/%/%25/g' | awk 'BEGIN{ORS="%0A"}{print}')
  echo "::error title=verify failed (exit $code)::${body}"
fi
exit "$code"
