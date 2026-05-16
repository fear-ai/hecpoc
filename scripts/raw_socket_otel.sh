#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ADDR="${OTEL_HEC_ADDR:-127.0.0.1:18089}"
TOKEN="${OTEL_HEC_TOKEN:-dev-token}"
OUT="${OTEL_RAW_SOCKET_OUT:-results/raw-socket-otel}"
CASE="${OTEL_RAW_SOCKET_CASE:-all}"
READ_TIMEOUT_MS="${OTEL_RAW_SOCKET_READ_TIMEOUT_MS:-8000}"
SLOW_BODY_DELAY_MS="${OTEL_RAW_SOCKET_SLOW_BODY_DELAY_MS:-6000}"

if ! nc -z "${ADDR%:*}" "${ADDR##*:}" >/dev/null 2>&1; then
  cat >&2 <<EOF
OpenTelemetry Collector HEC receiver is not reachable at $ADDR.
Start it first, for example:

  otelcol-contrib --config scripts/config/otel-splunk-hec.yaml

or use a Docker image / locally built collector with the same config.
EOF
  exit 2
fi

if [[ "${RAW_SOCKET_RELEASE:-0}" != "0" ]]; then
  cargo build --release --bin raw_socket_hec >/dev/null
  BIN="$ROOT/target/release/raw_socket_hec"
else
  cargo build --bin raw_socket_hec >/dev/null
  BIN="$ROOT/target/debug/raw_socket_hec"
fi

exec "$BIN" \
  --addr "$ADDR" \
  --token "$TOKEN" \
  --out "$OUT" \
  --case "$CASE" \
  --read-timeout-ms "$READ_TIMEOUT_MS" \
  --slow-body-delay-ms "$SLOW_BODY_DELAY_MS"
