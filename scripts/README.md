# HECpoc Scripts

This directory contains local validation, Splunk exploration, and benchmark-support scripts. These scripts are evidence tools, not product runtime components.

## Script Inventory

| Script | Category | Purpose | Current Limitations |
|---|---|---|---|
| `curl_hec_matrix.sh` | HECpoc/Splunk live HTTP matrix | Sends a broad curl-driven HEC compatibility matrix, including auth, endpoint, JSON/raw, gzip, index, health, and route cases; records status, headers, body, payloads, and summary. | Uses curl, so malformed wire/framing cases remain out of scope; disabled-token case only runs when a disabled token is supplied. |
| `verify_splunk_hec.sh` | Splunk exploration / oracle capture | Sends selected HEC cases to a live Splunk HEC endpoint and records status, headers, body, curl errors, payloads, and `summary.tsv`. | Does not assert expected values; curl cannot reliably craft every malformed wire condition because it normalizes some headers such as `Content-Length`; raw-socket probes are still needed. |
| `raw_socket_hec.sh` | Exact-byte wire validation | Runs the Rust `raw_socket_hec` verifier as a command; use `--help` or `--list-cases` for supported arguments, defaults, numbered cases, and artifact layout. | Plain TCP only; TLS support is postponed, so direct Splunk `https://127.0.0.1:8088` probing still needs curl or a future TLS mode. |
| `raw_socket_otel.sh` | Exact-byte OTel comparison | Runs the same Rust raw-socket verifier against an OpenTelemetry Collector Splunk HEC receiver, default `127.0.0.1:18089`. | Comparison target for design/tool hardening, not the Splunk compatibility oracle. Start OTel first with `scripts/config/otel-splunk-hec.yaml`. |
| `bench_hec_ab.sh` | HECpoc benchmark validation | Builds release binary, starts local receiver, runs ApacheBench single/concurrent raw uploads, captures stats before/after, launches system monitor, writes benchmark summary. | Uses `ab`; measures localhost HTTP/drop-sink path only; not a durability, TLS, ACK, or indexing benchmark. |
| `analyze_bench_run.py` | Benchmark analysis | Parses AB output and HEC stats snapshots into receiver-side requests/sec, MiB/sec, events/sec, and failure counters. | AB-output parsing is format-specific and should be extended carefully if other load tools are added. |
| `capture_system_stats.sh` | Benchmark/system evidence | Samples process CPU, memory, threads/LWP, descriptors, `top`, VM, `netstat`, `iostat`, and thread listings for a target PID. | Cross-platform best-effort shell script; network grep patterns are tuned for current HEC port ranges and should become parameterized before broader use. |
| `capture_net_observe.sh` | Network observation | Samples `netstat`, `lsof`, selected `sysctl` values, `ulimit`, and stats endpoint for repeated network-observation runs. | macOS-oriented defaults; default port may not match the active receiver; stats endpoint must be supplied when running non-default ports. |

## Config Inventory

| Config | Purpose | Invocation |
|---|---|---|
| `config/otel-splunk-hec.yaml` | OpenTelemetry Collector contrib Splunk HEC receiver on port `18089`, debug exporter | `otelcol-contrib --config scripts/config/otel-splunk-hec.yaml` or Docker image equivalent |
| `config/vector-hec-receiver.yaml` | Vector Splunk HEC receiver on port `18090`, console JSON output | `vector --config scripts/config/vector-hec-receiver.yaml` |
| `config/vector-to-hecpoc-raw.yaml` | Vector stdin source to HECpoc raw endpoint on `127.0.0.1:18088` | `vector --config scripts/config/vector-to-hecpoc-raw.yaml` |

These files are integration-harness assets. They are kept with scripts because they launch third-party tools for evidence capture; they are not Rust unit tests and do not define product runtime defaults.

## Use Categories

### Splunk Exploration

Use when HECpoc behavior is unclear because Splunk documentation is vague or incomplete.

Primary script:

```sh
SPLUNK_HEC_TOKEN='<token>' \
SPLUNK_HEC_URL='https://127.0.0.1:8088' \
SPLUNK_HEC_INSECURE=1 \
./scripts/verify_splunk_hec.sh
```

Output is evidence. It should be copied into documentation only as summarized findings with result directory references.

The broader curl matrix can also target Splunk:

```sh
HEC_MATRIX_URL='https://127.0.0.1:8088' \
HEC_MATRIX_TOKEN='<token>' \
HEC_MATRIX_INSECURE=1 \
./scripts/curl_hec_matrix.sh
```

For local raw-socket oracle work, Splunk Enterprise HEC can be configured for plain HTTP by setting HEC SSL off in Splunk Web global HEC settings or by using `enableSSL = 0` for the HEC `[http]` input. Use that only for local testing; production HEC should remain TLS-protected.

### HECpoc Added-Function Validation

Use local Rust tests for new HECpoc behavior before comparing to Splunk:

- parser edge cases;
- handler response bodies;
- reporting facts and counters;
- config validation;
- health phase behavior.

Use the same curl matrix against a local HECpoc server for live-router confirmation:

```sh
HEC_MATRIX_URL='http://127.0.0.1:18088' \
HEC_MATRIX_TOKEN='dev-token' \
HEC_MATRIX_DISABLED_TOKEN='disabled-token' \
./scripts/curl_hec_matrix.sh
```

### Performance And Load Evidence

Use `bench_hec_ab.sh` and `capture_system_stats.sh` for repeatable local throughput and process-resource measurements. Benchmark claims must name payload, request count, concurrency, sink mode, binary profile, and result directory.

`bench_hec_ab.sh` uses `OBSERVE_LEVEL` for the global tracing level and `OBSERVE_SOURCES` for comma-separated per-source overrides, passed through `HEC_OBSERVE_SOURCES`.

## Raw Socket Gap

Some conditions cannot be reliably produced by curl or AB:

- partial headers;
- slowloris header stall;
- malformed `Content-Length` that the client library would correct or reject;
- truncated chunked body;
- header sent with no body followed by idle timeout.

Use `scripts/raw_socket_hec.sh` for these cases:

```sh
scripts/raw_socket_hec.sh --help
scripts/raw_socket_hec.sh --list-cases
scripts/raw_socket_hec.sh --addr 127.0.0.1:18088 --token dev-token --case all
```

Defaults are `--addr 127.0.0.1:18088`, `--token dev-token`, `--case all`, `--read-timeout-ms 8000`, and `--slow-body-delay-ms 6000`.

The command creates a unique UTC timestamp run directory below `--out`, such as `results/raw-socket/20260515T181007Z`, adding a numeric suffix when multiple invocations start in the same second, and writes `show-config.log`, numbered request artifacts, response artifacts, summary, and JSON results there. Cases marked stack-owned may be rejected or timed out before the HEC handler exists in Axum/Hyper request form, so their absence from HEC handler stats is expected evidence rather than automatic receiver failure.

For local HECpoc runs, add `--stats-url http://HOST:PORT/hec/stats` to capture before/after stats per case. With stats enabled, handler-visible cases can report simple `Pass` or `Fail`; stack-owned cases remain `Info` or `Review` because they require socket, server log, or OS-level interpretation.

For OpenTelemetry Collector comparison runs:

```sh
otelcol-contrib --config scripts/config/otel-splunk-hec.yaml
scripts/raw_socket_otel.sh
```

`raw_socket_otel.sh` defaults to `OTEL_HEC_ADDR=127.0.0.1:18089`, `OTEL_HEC_TOKEN=dev-token`, `OTEL_RAW_SOCKET_CASE=all`, and `OTEL_RAW_SOCKET_OUT=results/raw-socket-otel`. The script intentionally records OTel behavior as comparison evidence: useful during design and tool honing, but not a substitute for Splunk oracle runs.
