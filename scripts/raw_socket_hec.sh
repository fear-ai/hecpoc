#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "${RAW_SOCKET_RELEASE:-0}" != "0" ]]; then
  cargo build --release --bin raw_socket_hec >/dev/null
  exec "$ROOT/target/release/raw_socket_hec" "$@"
fi

exec cargo run --quiet --bin raw_socket_hec -- "$@"
