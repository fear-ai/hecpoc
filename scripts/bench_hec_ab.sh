#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

PAYLOAD=${PAYLOAD:-/Users/walter/Work/Spank/Logs/loghub/Apache_2k.log}
TOKEN=${TOKEN:-dev-token}
PORT=${PORT:-18440}
ADDR=${ADDR:-127.0.0.1:$PORT}
RESULTS_ROOT=${RESULTS_ROOT:-$ROOT/results}
RUN=${RUN:-$RESULTS_ROOT/bench-hec-$(date -u +%Y%m%dT%H%M%SZ)}
MAX_BYTES=${MAX_BYTES:-30000000}
MAX_DECODED_BYTES=${MAX_DECODED_BYTES:-60000000}
MAX_EVENTS=${MAX_EVENTS:-1000000}
C1_REQUESTS=${C1_REQUESTS:-500}
CN_REQUESTS=${CN_REQUESTS:-2000}
CONCURRENCY=${CONCURRENCY:-16}
MONITOR_INTERVAL=${MONITOR_INTERVAL:-2}
OBSERVE_LEVEL=${OBSERVE_LEVEL:-warn}
OBSERVE_SOURCES=${OBSERVE_SOURCES:-hec.receiver=warn,hec.body=warn,hec.parser=warn,hec.sink=warn}

mkdir -p "$RUN"
cp "$PAYLOAD" "$RUN/payload.input"
wc -c "$PAYLOAD" > "$RUN/payload.bytes"
wc -l "$PAYLOAD" > "$RUN/payload.lines"
{
  echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "payload=$PAYLOAD"
  echo "addr=$ADDR"
  echo "c1_requests=$C1_REQUESTS"
  echo "cN_requests=$CN_REQUESTS"
  echo "concurrency=$CONCURRENCY"
  rustc -vV 2>/dev/null || true
  git status --short 2>/dev/null || true
} > "$RUN/manifest.txt"

cargo build --release

HEC_OBSERVE_SOURCES="$OBSERVE_SOURCES" target/release/hec-receiver \
  --addr "$ADDR" \
  --token "$TOKEN" \
  --max-bytes "$MAX_BYTES" \
  --max-decoded-bytes "$MAX_DECODED_BYTES" \
  --max-events "$MAX_EVENTS" \
  --observe-level "$OBSERVE_LEVEL" \
  --observe-format json \
  --observe-tracing true \
  --observe-console false \
  --observe-stats true \
  >"$RUN/server.stdout" 2>"$RUN/server.stderr" &
SERVER_PID=$!
echo "$SERVER_PID" > "$RUN/server.pid"
cleanup() {
  kill "$SERVER_PID" 2>/dev/null || true
  wait "$SERVER_PID" 2>/dev/null || true
  if [[ -n "${MONITOR_PID:-}" ]]; then
    kill "$MONITOR_PID" 2>/dev/null || true
    wait "$MONITOR_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

for _ in {1..100}; do
  if curl -sS "http://$ADDR/services/collector/health" > "$RUN/health.json" 2>"$RUN/health.err"; then
    break
  fi
  sleep 0.1
done

scripts/capture_system_stats.sh --pid "$SERVER_PID" --out "$RUN/system" --interval "$MONITOR_INTERVAL" &
MONITOR_PID=$!

curl -sS "http://$ADDR/hec/stats" > "$RUN/stats-before.json"
/usr/bin/time -p ab -n "$C1_REQUESTS" -c 1 -p "$PAYLOAD" -T 'text/plain' -H "Authorization: Splunk $TOKEN" "http://$ADDR/services/collector/raw" > "$RUN/ab-n${C1_REQUESTS}-c1.txt" 2> "$RUN/time-ab-n${C1_REQUESTS}-c1.txt"
curl -sS "http://$ADDR/hec/stats" > "$RUN/stats-after-c1.json"
/usr/bin/time -p ab -n "$CN_REQUESTS" -c "$CONCURRENCY" -p "$PAYLOAD" -T 'text/plain' -H "Authorization: Splunk $TOKEN" "http://$ADDR/services/collector/raw" > "$RUN/ab-n${CN_REQUESTS}-c${CONCURRENCY}.txt" 2> "$RUN/time-ab-n${CN_REQUESTS}-c${CONCURRENCY}.txt"
curl -sS "http://$ADDR/hec/stats" > "$RUN/stats-after-cN.json"

scripts/analyze_bench_run.py "$RUN" | tee "$RUN/summary.stdout.json"
printf 'run_dir=%s\n' "$RUN"
