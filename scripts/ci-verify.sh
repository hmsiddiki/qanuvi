#!/usr/bin/env bash
# CI: runs `npm run verify`; when it fails, its failing lines (or its last lines) are posted as an annotation
# (::error::), which the run's summary page and the checks API show without opening the log.
set -uo pipefail
cd "$(dirname "$0")/.."

npm run verify 2>&1 | tee /tmp/qanuvi-verify-raw.txt
code=${PIPESTATUS[0]}
if [ "$code" -ne 0 ]; then
  # Colour codes and carriage returns out: the search then sees plain lines, and a carriage return can no longer end
  # the annotation early (it did on macOS, so the failing test's name never showed).
  esc=$(printf '\033')
  tr -d '\r' < /tmp/qanuvi-verify-raw.txt | sed "s/${esc}\[[0-9;?]*[A-Za-z]//g" > /tmp/qanuvi-verify.txt
  encode() { sed 's/%/%25/g' | awk 'BEGIN{ORS="%0A"}{print}'; }
  # The failing Rust tests by name, with their panic lines, in an annotation of their own (short, so never cut).
  names=$(grep -E "^test .* \.\.\. FAILED$|panicked at|^error: test failed" /tmp/qanuvi-verify.txt | head -n 30)
  [ -z "$names" ] || echo "::error title=failing Rust tests::$(printf '%s' "$names" | encode)"
  # Rust and vitest failures first; otherwise the end of the output.
  lines=$(grep -E "FAILED|panicked|failures:|FAIL |AssertionError|^error|ERR!|Error:" /tmp/qanuvi-verify.txt | tail -n 40)
  [ -n "$lines" ] || lines=$(tail -n 60 /tmp/qanuvi-verify.txt)
  body=$(printf '%s\n--- end of output ---\n%s' "$lines" "$(tail -n 20 /tmp/qanuvi-verify.txt)" | encode)
  echo "::error title=verify failed (exit $code)::${body}"
fi
exit "$code"
