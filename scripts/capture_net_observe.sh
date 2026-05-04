#!/usr/bin/env bash
set -euo pipefail

host="${HEC_OBSERVE_HOST:-127.0.0.1}"
port="${HEC_OBSERVE_PORT:-18194}"
stats_url="${HEC_OBSERVE_STATS_URL:-http://${host}:${port}/hec/stats}"
interval="${HEC_OBSERVE_INTERVAL:-3}"
samples="${HEC_OBSERVE_SAMPLES:-20}"
out_dir="${HEC_OBSERVE_OUT:-observe/$(date -u +%Y%m%dT%H%M%SZ)}"

mkdir -p "$out_dir"

run_capture() {
  local name="$1"
  shift
  {
    printf 'timestamp_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'command='
    printf '%q ' "$@"
    printf '\n'
    "$@" 2>&1 || true
  } >> "$out_dir/$name.log"
}

run_shell_capture() {
  local name="$1"
  local command="$2"
  {
    printf 'timestamp_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'command=%s\n' "$command"
    eval "$command" 2>&1 || true
  } >> "$out_dir/$name.log"
}

printf 'capture_dir=%s\n' "$out_dir" | tee "$out_dir/manifest.txt"
printf 'host=%s\nport=%s\nstats_url=%s\ninterval=%s\nsamples=%s\n' \
  "$host" "$port" "$stats_url" "$interval" "$samples" >> "$out_dir/manifest.txt"

for sample in $(seq 1 "$samples"); do
  timestamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf '%s sample=%s/%s\n' "$timestamp" "$sample" "$samples" | tee -a "$out_dir/manifest.txt"

  {
    printf 'timestamp_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'command=netstat -anv -p tcp | awk -v endpoint=%q ...\n' "${host}.${port}"
    netstat -anv -p tcp | awk -v endpoint="${host}.${port}" \
      'index($0, endpoint) { states[$6]++ } END { for (state in states) print state, states[state] }' 2>&1 || true
  } >> "$out_dir/netstat_states.log"
  run_shell_capture "netstat_raw" "netstat -anv -p tcp | grep '${host}.${port}'"
  run_capture "lsof_port" lsof -nP -iTCP:"$port"
  run_shell_capture "sysctl_network" \
    "sysctl kern.ipc.somaxconn kern.ipc.maxsockets net.inet.ip.portrange.first net.inet.ip.portrange.last net.inet.tcp.msl net.inet.tcp.keepidle net.inet.tcp.keepintvl net.inet.tcp.keepcnt"
  run_shell_capture "ulimit" "ulimit -a"
  run_shell_capture "stats" "curl -fsS '$stats_url'"

  if command -v jq >/dev/null 2>&1; then
    tail -1 "$out_dir/stats.log" | jq . >> "$out_dir/stats.pretty.jsonl" 2>/dev/null || true
  fi

  sleep "$interval"
done

printf 'done=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee -a "$out_dir/manifest.txt"
