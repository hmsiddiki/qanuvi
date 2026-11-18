#!/usr/bin/env bash
# CI on Linux: runs every Rust test on its own, inside a cgroup with a memory, process and time limit. A test that
# runs away (memory, a process storm, a hang) then fails by name instead of taking the whole runner down, which
# loses the log ("the hosted runner lost communication").
#
# Compile errors and failed tests are also posted as annotations (::error::), which the run's summary page and the
# checks API show without opening the log.
set -uo pipefail
cd "$(dirname "$0")/../src-tauri"

# One annotation: newlines and % as GitHub's workflow commands want them, at most 60 lines.
annotate() {
  local title body
  title=$(printf '%s' "$1" | sed 's/%/%25/g; s/:/%3A/g; s/,/%2C/g')
  body=$(printf '%s' "$2" | tr -d '\r' | tail -n 60 | sed 's/%/%25/g' | awk 'BEGIN{ORS="%0A"}{print}')
  echo "::error title=${title}::${body}"
}

cargo test --no-run --message-format=json 2>/tmp/qanuvi-build.txt > /tmp/qanuvi-build.json
build=$?
node -e 'let s=require("fs").readFileSync("/tmp/qanuvi-build.json","utf8");const bins=[],errs=[];for(const l of s.split("\n")){try{const m=JSON.parse(l);if(m.reason==="compiler-artifact"&&m.executable&&m.profile&&m.profile.test)bins.push(m.executable);if(m.reason==="compiler-message"&&m.message&&m.message.level==="error")errs.push(m.message.rendered)}catch{}}require("fs").writeFileSync("/tmp/qanuvi-test-bins.txt",bins.join("\n")+(bins.length?"\n":""));require("fs").writeFileSync("/tmp/qanuvi-build-errors.txt",errs.join("\n"))'
if [ "$build" -ne 0 ] || [ ! -s /tmp/qanuvi-test-bins.txt ]; then
  errors=$(cat /tmp/qanuvi-build-errors.txt)
  [ -n "$errors" ] || errors=$(tail -n 60 /tmp/qanuvi-build.txt)
  echo "$errors"
  annotate "Rust tests did not build (exit $build)" "$errors"
  exit 1
fi

failed=()
while read -r bin; do
  while read -r name; do
    [ -n "$name" ] || continue
    echo "::group::$name"
    start=$(date +%s)
    sudo --preserve-env env "PATH=$PATH" "HOME=$HOME" systemd-run --scope --quiet --uid="$(id -u)" --gid="$(id -g)" \
      -p MemoryMax=3G -p MemorySwapMax=0 -p TasksMax=256 \
      timeout -k 5 180 "$bin" --exact "$name" --test-threads=1 --nocapture </dev/null 2>&1 | tee /tmp/qanuvi-test-out.txt
    code=${PIPESTATUS[0]}
    echo "::endgroup::"
    echo "$name: exit $code in $(( $(date +%s) - start ))s; free memory $(free -m | awk '/Mem:/{print $7}') MB"
    if [ "$code" -ne 0 ]; then
      failed+=("$name (exit $code)")
      annotate "Rust test failed: $name (exit $code)" "$(cat /tmp/qanuvi-test-out.txt)"
    fi
  done < <("$bin" --list --format=terse 2>/dev/null | sed -n 's/: test$//p')
done < /tmp/qanuvi-test-bins.txt

if [ "${#failed[@]}" -gt 0 ]; then
  echo "Failed or stopped tests (exit 124 = time limit, 137 = killed, often the memory limit):"
  printf '  %s\n' "${failed[@]}"
  exit 1
fi
echo "All Rust tests passed one by one."
