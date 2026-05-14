#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${HEC_MATRIX_URL:-http://127.0.0.1:18088}"
TOKEN="${HEC_MATRIX_TOKEN:-dev-token}"
WRONG_TOKEN="${HEC_MATRIX_WRONG_TOKEN:-wrong-token}"
DISABLED_TOKEN="${HEC_MATRIX_DISABLED_TOKEN:-}"
OUT_DIR="${HEC_MATRIX_OUT:-results/hec-curl-matrix-$(date -u +%Y%m%dT%H%M%SZ)}"
MAX_TIME="${HEC_MATRIX_MAX_TIME:-20}"
INSECURE="${HEC_MATRIX_INSECURE:-0}"

CURL_INSECURE=()
if [[ "$INSECURE" != "0" ]]; then
  CURL_INSECURE=(-k)
fi

mkdir -p "$OUT_DIR"/{payloads,responses,headers,errors}
summary="$OUT_DIR/summary.tsv"
printf 'case\tmethod\tpath\tstatus\tbody_file\tnotes\n' > "$summary"

cat > "$OUT_DIR/manifest.txt" <<MANIFEST
base_url=$BASE_URL
out_dir=$OUT_DIR
max_time=$MAX_TIME
date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
disabled_token_present=$([[ -n "$DISABLED_TOKEN" ]] && echo true || echo false)
MANIFEST

write_payload() {
  local name="$1"
  local payload="$2"
  printf '%s' "$payload" > "$OUT_DIR/payloads/$name.body"
}

write_binary_payloads() {
  python3 - "$OUT_DIR/payloads" <<'PY'
import gzip
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
(root / "empty.body").write_bytes(b"")
(root / "malformed_gzip.body").write_bytes(b"not a gzip stream")
(root / "gzip_ok.body").write_bytes(gzip.compress(b"one\ntwo\n"))
(root / "gzip_expansion.body").write_bytes(gzip.compress(b"abcdef"))
(root / "invalid_utf8_raw.body").write_bytes(b"a\xffb\n")
PY
}

run_case() {
  local name="$1"
  local method="$2"
  local path="$3"
  local body_file="$4"
  local auth="$5"
  local notes="$6"
  shift 6
  local header_args=("$@")
  local response="$OUT_DIR/responses/$name.body"
  local headers="$OUT_DIR/headers/$name.headers"
  local error="$OUT_DIR/errors/$name.err"
  local auth_args=()
  local status rc

  case "$auth" in
    good) auth_args=(-H "Authorization: Splunk $TOKEN") ;;
    wrong) auth_args=(-H "Authorization: Splunk $WRONG_TOKEN") ;;
    disabled) auth_args=(-H "Authorization: Splunk $DISABLED_TOKEN") ;;
    basic) auth_args=(-u "user:$TOKEN") ;;
    bearer) auth_args=(-H "Authorization: Bearer $TOKEN") ;;
    blank) auth_args=(-H "Authorization:") ;;
    none) auth_args=() ;;
    *) echo "unknown auth mode: $auth" >&2; exit 2 ;;
  esac

  set +e
  status=$(curl "${CURL_INSECURE[@]}" -sS \
    --max-time "$MAX_TIME" \
    -X "$method" \
    "${auth_args[@]}" \
    "${header_args[@]}" \
    -D "$headers" \
    -o "$response" \
    -w '%{http_code}' \
    --data-binary "@$body_file" \
    "$BASE_URL$path" 2>"$error")
  rc=$?
  set -e

  if [[ $rc -ne 0 ]]; then
    status="curl_rc_$rc"
  fi
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$method" "$path" "$status" "$response" "$notes" | tee -a "$summary"
}

run_get() {
  local name="$1"
  local path="$2"
  local auth="$3"
  local notes="$4"
  local response="$OUT_DIR/responses/$name.body"
  local headers="$OUT_DIR/headers/$name.headers"
  local error="$OUT_DIR/errors/$name.err"
  local auth_args=()
  local status rc

  case "$auth" in
    good) auth_args=(-H "Authorization: Splunk $TOKEN") ;;
    none) auth_args=() ;;
    *) echo "unknown auth mode: $auth" >&2; exit 2 ;;
  esac

  set +e
  status=$(curl "${CURL_INSECURE[@]}" -sS \
    --max-time "$MAX_TIME" \
    "${auth_args[@]}" \
    -D "$headers" \
    -o "$response" \
    -w '%{http_code}' \
    "$BASE_URL$path" 2>"$error")
  rc=$?
  set -e

  if [[ $rc -ne 0 ]]; then
    status="curl_rc_$rc"
  fi
  printf '%s\tGET\t%s\t%s\t%s\t%s\n' "$name" "$path" "$status" "$response" "$notes" | tee -a "$summary"
}

write_payload event_ok '{"event":"ok","host":"verify-host","source":"verify-source","sourcetype":"verify:sourcetype","fields":{"alpha":"a","n":1,"flag":true,"none":null}}'
write_payload stacked '{"event":"one"}{"event":"two","fields":{"k":"v"}}'
write_payload json_array '[{"event":"a"},{"event":"b"}]'
write_payload missing_event '{"host":"verify-host"}'
write_payload blank_event '{"event":""}'
write_payload malformed_missing_brace '{"event":"unterminated"'
write_payload malformed_trailing_garbage '{"event":"ok"}xyz'
write_payload invalid_index '{"event":"x","index":"Bad.Index"}'
write_payload fields_nested_object '{"event":"x","fields":{"nested":{"x":1}}}'
write_payload fields_array_value '{"event":"x","fields":{"roles":["admin"]}}'
write_payload fields_top_array '{"event":"x","fields":["not","object"]}'
write_payload raw_ok $'one\ntwo\n'
write_payload raw_blank $'\n\r\n'
write_payload raw_final_no_lf 'final-without-newline'
write_payload raw_spaces $' \t \n'
write_payload oversize_small 'abcdef'
write_payload ack_query '{"acks":[0,1]}'
write_binary_payloads

run_case event_ok POST /services/collector/event "$OUT_DIR/payloads/event_ok.body" good "baseline event with metadata and fields"
run_case stacked POST /services/collector/event "$OUT_DIR/payloads/stacked.body" good "stacked JSON objects"
run_case json_array POST /services/collector/event "$OUT_DIR/payloads/json_array.body" good "JSON array HEC batch"
run_case event_empty_body POST /services/collector/event "$OUT_DIR/payloads/empty.body" good "empty event body"
run_case missing_event POST /services/collector/event "$OUT_DIR/payloads/missing_event.body" good "missing event field"
run_case blank_event POST /services/collector/event "$OUT_DIR/payloads/blank_event.body" good "blank event field"
run_case malformed_missing_brace POST /services/collector/event "$OUT_DIR/payloads/malformed_missing_brace.body" good "unterminated object"
run_case malformed_trailing_garbage POST /services/collector/event "$OUT_DIR/payloads/malformed_trailing_garbage.body" good "valid event followed by garbage"
run_case invalid_index POST /services/collector/event "$OUT_DIR/payloads/invalid_index.body" good "invalid event index"
run_case fields_nested_object POST /services/collector/event "$OUT_DIR/payloads/fields_nested_object.body" good "nested indexed field object"
run_case fields_array_value POST /services/collector/event "$OUT_DIR/payloads/fields_array_value.body" good "array indexed field value"
run_case fields_top_array POST /services/collector/event "$OUT_DIR/payloads/fields_top_array.body" good "fields value is not object"

run_case raw_ok POST /services/collector/raw "$OUT_DIR/payloads/raw_ok.body" good "baseline raw lines"
run_case raw_blank POST /services/collector/raw "$OUT_DIR/payloads/raw_blank.body" good "blank raw body"
run_case raw_spaces POST /services/collector/raw "$OUT_DIR/payloads/raw_spaces.body" good "whitespace raw body"
run_case raw_final_no_lf POST /services/collector/raw "$OUT_DIR/payloads/raw_final_no_lf.body" good "raw final line without LF"
run_case raw_invalid_utf8 POST /services/collector/raw "$OUT_DIR/payloads/invalid_utf8_raw.body" good "raw invalid UTF-8"

run_case no_auth POST /services/collector/raw "$OUT_DIR/payloads/raw_ok.body" none "missing Authorization"
run_case blank_auth POST /services/collector/raw "$OUT_DIR/payloads/raw_ok.body" blank "blank Authorization"
run_case bearer_auth POST /services/collector/raw "$OUT_DIR/payloads/raw_ok.body" bearer "unsupported Bearer scheme"
run_case wrong_token POST /services/collector/raw "$OUT_DIR/payloads/raw_ok.body" wrong "unknown token"
run_case basic_auth POST /services/collector/raw "$OUT_DIR/payloads/raw_ok.body" basic "Basic auth password token"
if [[ -n "$DISABLED_TOKEN" ]]; then
  run_case disabled_token POST /services/collector/raw "$OUT_DIR/payloads/raw_ok.body" disabled "disabled token"
fi

run_case unsupported_encoding POST /services/collector/raw "$OUT_DIR/payloads/raw_ok.body" good "unsupported content encoding" -H "Content-Encoding: br"
run_case gzip_ok POST /services/collector/raw "$OUT_DIR/payloads/gzip_ok.body" good "valid gzip raw body" -H "Content-Encoding: gzip"
run_case malformed_gzip POST /services/collector/raw "$OUT_DIR/payloads/malformed_gzip.body" good "malformed gzip stream" -H "Content-Encoding: gzip"
run_case oversize_advertised POST /services/collector/raw "$OUT_DIR/payloads/oversize_small.body" good "advertised oversize content length; curl may rewrite" -H "Content-Length: 999999999"

run_case ack_disabled POST /services/collector/ack "$OUT_DIR/payloads/ack_query.body" good "ACK status query"
run_get health /services/collector/health none "health endpoint"
run_get unknown_path /services/collector/not-a-real-endpoint good "unknown HEC route"
run_get get_raw /services/collector/raw good "wrong method on raw route"

if curl "${CURL_INSECURE[@]}" -fsS --max-time "$MAX_TIME" "$BASE_URL/hec/stats" > "$OUT_DIR/stats.json" 2>"$OUT_DIR/stats.err"; then
  printf 'stats\tGET\t/hec/stats\t200\t%s\tlocal HECpoc stats endpoint\n' "$OUT_DIR/stats.json" | tee -a "$summary"
fi

echo "summary: $summary"
