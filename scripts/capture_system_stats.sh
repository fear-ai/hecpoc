#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: capture_system_stats.sh --pid PID --out DIR [--interval SECONDS] [--duration SECONDS]

Samples process and system state for long HEC benchmark runs. Output files are
append-only TSV/text files suitable for later parsing.
EOF
}

PID=""
OUT=""
INTERVAL="${INTERVAL:-2}"
DURATION="${DURATION:-0}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --pid) PID="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --interval) INTERVAL="$2"; shift 2 ;;
    --duration) DURATION="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ -z "$PID" || -z "$OUT" ]]; then
  usage >&2
  exit 2
fi

mkdir -p "$OUT"
META="$OUT/system_meta.txt"
PROC_TSV="$OUT/process.tsv"
FD_TSV="$OUT/fd.tsv"
TOP_TXT="$OUT/top.txt"
VM_TXT="$OUT/vm.txt"
NET_TXT="$OUT/netstat.txt"
IO_TXT="$OUT/iostat.txt"
THREAD_TXT="$OUT/threads.txt"

{
  echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "pid=$PID"
  echo "uname=$(uname -a)"
  command -v sysctl >/dev/null && {
    sysctl -n hw.ncpu 2>/dev/null | sed 's/^/hw.ncpu=/' || true
    sysctl -n hw.memsize 2>/dev/null | sed 's/^/hw.memsize=/' || true
    sysctl -n machdep.cpu.brand_string 2>/dev/null | sed 's/^/cpu=/' || true
    sysctl -n kern.maxfiles 2>/dev/null | sed 's/^/kern.maxfiles=/' || true
    sysctl -n kern.maxfilesperproc 2>/dev/null | sed 's/^/kern.maxfilesperproc=/' || true
  }
  command -v ulimit >/dev/null && ulimit -a || true
} > "$META"

printf 'timestamp_utc\tpid\tppid\tpcpu\tpmem\trss_kb\tvsz_kb\tthreads_or_lwp\tetime\tcommand\n' > "$PROC_TSV"
printf 'timestamp_utc\tfd_count\n' > "$FD_TSV"

start_epoch=$(date +%s)
while kill -0 "$PID" 2>/dev/null; do
  now_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  now_epoch=$(date +%s)

  if ps -p "$PID" -o pid=,ppid=,%cpu=,%mem=,rss=,vsz=,thcount=,etime=,command= >/tmp/hec-ps-$$ 2>/dev/null; then
    awk -v ts="$now_utc" '{$1=$1; print ts "\t" $0}' /tmp/hec-ps-$$ | sed 's/[[:space:]][[:space:]]*/\t/g' >> "$PROC_TSV"
  elif ps -p "$PID" -o pid=,ppid=,%cpu=,%mem=,rss=,vsz=,nlwp=,etime=,command= >/tmp/hec-ps-$$ 2>/dev/null; then
    awk -v ts="$now_utc" '{$1=$1; print ts "\t" $0}' /tmp/hec-ps-$$ | sed 's/[[:space:]][[:space:]]*/\t/g' >> "$PROC_TSV"
  fi
  rm -f /tmp/hec-ps-$$

  if command -v lsof >/dev/null; then
    fd_count=$(lsof -p "$PID" 2>/dev/null | awk 'NR>1 {count++} END {print count+0}')
    printf '%s\t%s\n' "$now_utc" "$fd_count" >> "$FD_TSV"
  fi

  {
    echo "--- $now_utc ---"
    if [[ "$(uname -s)" == "Darwin" ]]; then
      top -l 1 -pid "$PID" -stats pid,command,cpu,mem,threads,ports,time 2>/dev/null || true
    else
      top -b -n 1 -p "$PID" 2>/dev/null || true
    fi
  } >> "$TOP_TXT"

  {
    echo "--- $now_utc ---"
    vm_stat 2>/dev/null || vmstat 1 2 2>/dev/null || true
  } >> "$VM_TXT"

  {
    echo "--- $now_utc ---"
    netstat -an 2>/dev/null | grep -E '(:18088|:18[0-9][0-9][0-9]|:8088|\.18088|\.18[0-9][0-9][0-9])' || true
  } >> "$NET_TXT"

  {
    echo "--- $now_utc ---"
    iostat 1 2 2>/dev/null || true
  } >> "$IO_TXT"

  {
    echo "--- $now_utc ---"
    ps -M "$PID" 2>/dev/null || ps -L -p "$PID" 2>/dev/null || true
  } >> "$THREAD_TXT"

  if [[ "$DURATION" != "0" && $((now_epoch - start_epoch)) -ge "$DURATION" ]]; then
    break
  fi
  sleep "$INTERVAL"
done
