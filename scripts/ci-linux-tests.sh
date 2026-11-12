#!/usr/bin/env bash
# CI on Linux: runs every Rust test on its own, inside a cgroup with a memory, process and time limit. A test that
# runs away (memory, a process storm, a hang) then fails by name instead of taking the whole runner down, which
# loses the log ("the hosted runner lost communication").
set -uo pipefail
cd "$(dirname "$0")/../src-tauri"

cargo test --no-run --message-format=json 2>/dev/null \
  | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{for(const l of s.split("\n")){try{const m=JSON.parse(l);if(m.reason==="compiler-artifact"&&m.executable&&m.profile&&m.profile.test)console.log(m.executable)}catch{}}})' \
  > /tmp/qanuvi-test-bins.txt
[ -s /tmp/qanuvi-test-bins.txt ] || { echo "no test binaries"; exit 1; }

failed=()
while read -r bin; do
  while read -r name; do
    [ -n "$name" ] || continue
    echo "::group::$name"
    start=$(date +%s)
    sudo --preserve-env env "PATH=$PATH" "HOME=$HOME" systemd-run --scope --quiet --uid="$(id -u)" --gid="$(id -g)" \
      -p MemoryMax=3G -p MemorySwapMax=0 -p TasksMax=256 \
      timeout -k 5 180 "$bin" --exact "$name" --test-threads=1 --nocapture </dev/null
    code=$?
    echo "::endgroup::"
    echo "$name: exit $code in $(( $(date +%s) - start ))s; free memory $(free -m | awk '/Mem:/{print $7}') MB"
    [ "$code" -eq 0 ] || failed+=("$name (exit $code)")
  done < <("$bin" --list --format=terse 2>/dev/null | sed -n 's/: test$//p')
done < /tmp/qanuvi-test-bins.txt

if [ "${#failed[@]}" -gt 0 ]; then
  echo "Failed or stopped tests (exit 124 = time limit, 137 = killed, often the memory limit):"
  printf '  %s\n' "${failed[@]}"
  exit 1
fi
echo "All Rust tests passed one by one."
