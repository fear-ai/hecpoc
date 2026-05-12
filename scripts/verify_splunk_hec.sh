#!/usr/bin/env bash
set -euo pipefail

# Verify gray HEC behavior against a live Splunk Enterprise/Cloud HEC endpoint.
# Required:
#   SPLUNK_HEC_TOKEN=...
# Optional:
#   SPLUNK_HEC_URL=https://127.0.0.1:8088
#   SPLUNK_HEC_INSECURE=1          # pass -k to curl for local self-signed Splunk
#   SPLUNK_HEC_OUT=results/splunk-verify-<timestamp>
#   SPLUNK_HEC_RUN_OPTIONAL=1      # include postponed/ambiguous cases such as JSON arrays and health
#   SPLUNK_HEC_MAX_TIME=20
#
# The script records actual HTTP status, response headers, response body, and curl diagnostics.
# It intentionally does not assert expected outcomes; local Splunk is the oracle.

if [[ -z "${SPLUNK_HEC_TOKEN:-}" ]]; then
  echo "SPLUNK_HEC_TOKEN is required" >&2
  exit 2
fi

BASE_URL="${SPLUNK_HEC_URL:-https://127.0.0.1:8088}"
OUT_DIR="${SPLUNK_HEC_OUT:-results/splunk-verify-$(date -u +%Y%m%dT%H%M%SZ)}"
MAX_TIME="${SPLUNK_HEC_MAX_TIME:-20}"
RUN_OPTIONAL="${SPLUNK_HEC_RUN_OPTIONAL:-0}"
CURL_INSECURE=()
if [[ "${SPLUNK_HEC_INSECURE:-1}" != "0" ]]; then
  CURL_INSECURE=(-k)
fi

mkdir -p "$OUT_DIR/payloads" "$OUT_DIR/responses" "$OUT_DIR/headers" "$OUT_DIR/errors"

summary="$OUT_DIR/summary.tsv"
printf 'case\tmethod\tpath\tstatus\tbody_file\tnotes\n' > "$summary"

cat > "$OUT_DIR/manifest.txt" <<MANIFEST
base_url=$BASE_URL
out_dir=$OUT_DIR
run_optional=$RUN_OPTIONAL
max_time=$MAX_TIME
date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
MANIFEST

write_payload() {
  local name="$1"
  local payload="$2"
  printf '%s' "$payload" > "$OUT_DIR/payloads/$name.body"
}

run_case() {
  local name="$1"
  local method="$2"
  local path="$3"
  local body_file="$4"
  local notes="$5"
  shift 5
  local header_args=("$@")
  local url="$BASE_URL$path"
  local response="$OUT_DIR/responses/$name.json"
  local headers="$OUT_DIR/headers/$name.headers"
  local error="$OUT_DIR/errors/$name.err"
  local status

  set +e
  status=$(curl "${CURL_INSECURE[@]}" -sS \
    --max-time "$MAX_TIME" \
    -X "$method" \
    -H "Authorization: Splunk $SPLUNK_HEC_TOKEN" \
    "${header_args[@]}" \
    -D "$headers" \
    -o "$response" \
    -w '%{http_code}' \
    --data-binary "@$body_file" \
    "$url" 2>"$error")
  local rc=$?
  set -e

  if [[ $rc -ne 0 ]]; then
    status="curl_rc_$rc"
  fi
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$method" "$path" "$status" "$response" "$notes" | tee -a "$summary"
}

run_get() {
  local name="$1"
  local path="$2"
  local notes="$3"
  local response="$OUT_DIR/responses/$name.json"
  local headers="$OUT_DIR/headers/$name.headers"
  local error="$OUT_DIR/errors/$name.err"
  local status

  set +e
  status=$(curl "${CURL_INSECURE[@]}" -sS \
    --max-time "$MAX_TIME" \
    -H "Authorization: Splunk $SPLUNK_HEC_TOKEN" \
    -D "$headers" \
    -o "$response" \
    -w '%{http_code}' \
    "$BASE_URL$path" 2>"$error")
  local rc=$?
  set -e

  if [[ $rc -ne 0 ]]; then
    status="curl_rc_$rc"
  fi
  printf '%s\tGET\t%s\t%s\t%s\t%s\n' "$name" "$path" "$status" "$response" "$notes" | tee -a "$summary"
}

write_payload event_ok '{"event":"ok","host":"verify-host","source":"verify-source","sourcetype":"verify:sourcetype","fields":{"alpha":"a","n":1,"flag":true,"none":null}}'
write_payload stacked '{"event":"one"}{"event":"two","fields":{"k":"v"}}'
write_payload missing_event '{"host":"verify-host"}'
write_payload blank_event '{"event":""}'
write_payload malformed_missing_brace '{"event":"unterminated"'
write_payload malformed_missing_quote '{"event":"unterminated}'
write_payload malformed_trailing_garbage '{"event":"ok"}xyz'
write_payload fields_nested_object '{"event":"x","fields":{"nested":{"x":1}}}'
write_payload fields_array_value '{"event":"x","fields":{"roles":["admin"]}}'
write_payload fields_top_array '{"event":"x","fields":["not","object"]}'
write_payload raw_ok $'one\ntwo\n'
write_payload raw_blank $'\n\r\n'
write_payload raw_final_no_lf 'final-without-newline'
write_payload json_array '[{"event":"a"},{"event":"b"}]'
write_payload oversize_small 'abcdef'
write_payload unsupported_encoding 'abc'
write_payload ack_query '{"acks":[0,1]}'

run_case event_ok POST /services/collector/event "$OUT_DIR/payloads/event_ok.body" 'baseline event with flat scalar fields'
run_case stacked POST /services/collector/event "$OUT_DIR/payloads/stacked.body" 'documented stacked JSON objects batch'
run_case missing_event POST /services/collector/event "$OUT_DIR/payloads/missing_event.body" 'event missing; expect code 12 class'
run_case blank_event POST /services/collector/event "$OUT_DIR/payloads/blank_event.body" 'event blank; expect code 13 class'
run_case malformed_missing_brace POST /services/collector/event "$OUT_DIR/payloads/malformed_missing_brace.body" 'unterminated object'
run_case malformed_missing_quote POST /services/collector/event "$OUT_DIR/payloads/malformed_missing_quote.body" 'unterminated string'
run_case malformed_trailing_garbage POST /services/collector/event "$OUT_DIR/payloads/malformed_trailing_garbage.body" 'valid object followed by garbage'
run_case fields_nested_object POST /services/collector/event "$OUT_DIR/payloads/fields_nested_object.body" 'indexed fields nested object'
run_case fields_array_value POST /services/collector/event "$OUT_DIR/payloads/fields_array_value.body" 'indexed fields array value'
run_case fields_top_array POST /services/collector/event "$OUT_DIR/payloads/fields_top_array.body" 'fields is not an object'
run_case raw_ok POST /services/collector/raw "$OUT_DIR/payloads/raw_ok.body" 'baseline raw lines'
run_case raw_blank POST /services/collector/raw "$OUT_DIR/payloads/raw_blank.body" 'blank raw lines'
run_case raw_final_no_lf POST /services/collector/raw "$OUT_DIR/payloads/raw_final_no_lf.body" 'raw final line without LF'
run_case unsupported_encoding POST /services/collector/raw "$OUT_DIR/payloads/unsupported_encoding.body" 'unsupported content-encoding br' -H 'Content-Encoding: br'
run_case malformed_content_length POST /services/collector/raw "$OUT_DIR/payloads/raw_ok.body" 'malformed content-length header' -H 'Content-Length: nope'
run_case oversize_advertised POST /services/collector/raw "$OUT_DIR/payloads/oversize_small.body" 'advertised content-length too large; curl may override body length, inspect result' -H 'Content-Length: 999999999'
run_case ack_disabled POST /services/collector/ack "$OUT_DIR/payloads/ack_query.body" 'ACK status query when token ACK is disabled or unavailable; expect code 14 class if token has ACK disabled'
run_get unknown_path /services/collector/not-a-real-endpoint 'incorrect HEC path'

if [[ "$RUN_OPTIONAL" == "1" ]]; then
  run_case json_array POST /services/collector/event "$OUT_DIR/payloads/json_array.body" 'OPTIONAL: JSON array batch; docs suggest not supported, verify only'
  run_get health /services/collector/health 'OPTIONAL: only verifies reachable healthy endpoint unless Splunk can be forced unhealthy'
  run_get health_v1 /services/collector/health/1.0 'OPTIONAL: health v1 alias'
fi

echo "summary: $summary"
