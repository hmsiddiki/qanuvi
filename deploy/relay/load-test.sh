#!/bin/bash
# 24rl R4 load test on the Linux laptop: relay + load tool on one machine, 50,000 pairs = 100,000 links.
# Writes everything to ~/r4-load/ ; run with nohup.
set -u
ulimit -n 1048576
B=~/src/qanuvi/src-tauri/target/release
OUT=~/r4-load
rm -rf "$OUT"; mkdir -p "$OUT"
cd "$OUT"
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 2 -subj "/CN=127.0.0.1" \
  -addext "subjectAltName=IP:127.0.0.1" -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature" -addext "extendedKeyUsage=serverAuth" -keyout key.pem -out cert.pem 2>/dev/null
PAIRS=${PAIRS:-50000}
HZ=$(getconf CLK_TCK)

start_relay() {
  "$B/qanuvi-relay" --listen 127.0.0.1:47391 --tls-listen 127.0.0.1:47492 --cert cert.pem --key key.pem "$@" \
    >>relay.log 2>&1 &
  RELAY=$!
  sleep 1
}

# Samples the relay every 10 s: RSS, threads, open files, CPU % of one core since the last sample.
sample() {
  local tag=$1 secs=$2 last=0 t=0
  while [ $t -lt $secs ]; do
    local st; st=$(cat /proc/$RELAY/stat 2>/dev/null) || { echo "$tag t=$t relay gone"; sleep 10; t=$((t+10)); continue; }
    local ticks; ticks=$(echo "$st" | awk '{print $14+$15}')
    local rss; rss=$(awk '/VmRSS/{print $2}' /proc/$RELAY/status)
    local thr; thr=$(awk '/Threads/{print $2}' /proc/$RELAY/status)
    local fds; fds=$(ls /proc/$RELAY/fd | wc -l)
    if [ $last -gt 0 ]; then
      echo "$tag t=$t rss_kb=$rss threads=$thr fds=$fds cpu_pct=$(( (ticks-last)*100/HZ/10 ))"
    else
      echo "$tag t=$t rss_kb=$rss threads=$thr fds=$fds"
    fi
    last=$ticks
    sleep 10; t=$((t+10))
  done
}

start_relay
echo "baseline rss_kb=$(awk '/VmRSS/{print $2}' /proc/$RELAY/status)" > samples.txt

# A: all idle (pings only), plain road.
"$B/examples/relay-load" --relay ws://127.0.0.1:47391 --pairs $PAIRS --active 0 --sources 8 --rate 2000 \
  --seconds 240 > A-load.txt 2>&1 &
LOAD=$!
sample A 240 >> samples.txt
wait $LOAD
sleep 5
echo "after-A rss_kb=$(awk '/VmRSS/{print $2}' /proc/$RELAY/status)" >> samples.txt

# A2: all idle again with pings every 300 s instead of 30 s (how much of the idle CPU is pings).
kill $RELAY; wait $RELAY 2>/dev/null; sleep 3
start_relay --ping-every 300
"$B/examples/relay-load" --relay ws://127.0.0.1:47391 --pairs $PAIRS --active 0 --sources 8 --rate 2000 \
  --seconds 180 > A2-load.txt 2>&1 &
LOAD=$!
sample A2 180 >> samples.txt
wait $LOAD
kill $RELAY; wait $RELAY 2>/dev/null; sleep 3
start_relay

# B: 500 pairs talking every 5 s (1,000 active links), plain road; relay restarted at about 150 s (C).
"$B/examples/relay-load" --relay ws://127.0.0.1:47391 --pairs $PAIRS --active 500 --every 5 --sources 8 \
  --rate 2000 --seconds 330 > B-load.txt 2>&1 &
LOAD=$!
sample B 150 >> samples.txt
echo "C restart at $(date +%T)" >> samples.txt
kill $RELAY; wait $RELAY 2>/dev/null
start_relay
sample C 170 >> samples.txt
wait $LOAD
kill $RELAY; wait $RELAY 2>/dev/null
sleep 3

# D: all idle over the relay's own TLS port.
start_relay
"$B/examples/relay-load" --relay wss://127.0.0.1:47492 --ca cert.pem --pairs $PAIRS --active 100 --every 5 \
  --sources 8 --rate 1000 --seconds 240 > D-load.txt 2>&1 &
LOAD=$!
sample D 240 >> samples.txt
wait $LOAD
kill $RELAY; wait $RELAY 2>/dev/null
echo DONE >> samples.txt
