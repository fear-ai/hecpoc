# Config — HEC Receiver Configuration Contract

This file defines the production configuration surface for the HEC receiver. It describes the intended configuration system, validation rules, operational reporting, and test expectations. `Stack.md` may explain why a setting exists; this file defines what the setting is and how it is accepted.

## Strategy

Configuration is one typed data model assembled from four sources:

1. compiled defaults;
2. TOML config file;
3. command-line flags;
4. environment variables.

Mainstay libraries:

- `clap` parses CLI arguments and provides help, version output, and shell-completion support;
- `serde` defines typed configuration structs;
- `figment` composes defaults, TOML, CLI overrides, and environment providers;
- `toml` is the operator-facing file format through Figment's TOML provider;
- `validator` or explicit `validate()` methods enforce field and cross-field invariants after extraction;
- `thiserror` or a local typed error enum reports configuration, validation, and startup errors without losing source context.

Precedence:

```text
compiled defaults < TOML file < command line < environment
```

Figment should implement this by provider merge order: defaults first, TOML second, CLI override provider third, prefixed environment provider last.

Rationale:

- defaults make local startup deterministic;
- TOML is the persistent operator-facing configuration;
- command-line flags are explicit launch-time overrides and support tests;
- environment variables are highest precedence for service managers, containers, CI, and emergency overrides.

Hot reload is not a near-term goal. The initial policy is frozen config: parse, merge, validate, log effective startup configuration, then run. Later runtime reconfiguration must be opt-in per field or per subsystem and must preserve the same validation and audit rules.

Every setting has exactly one canonical field name. TOML keys, CLI flags, and environment names derive from that field.

## Naming

| Source | Form | Example |
| --- | --- | --- |
| Rust field | snake_case | `max_decoded_bytes` |
| TOML key | section + snake_case | `limits.max_decoded_bytes` |
| CLI flag | kebab-case | `--max-decoded-bytes` |
| Env var | upper snake with `HEC_` prefix | `HEC_MAX_DECODED_BYTES` |

Avoid historical or implementation names in the external surface:

- no `POC`;
- no `_BODY` when the real concept is wire bytes or decoded bytes;
- no `_MILLIS`; durations use strings such as `250ms`, `5s`, `2m`;
- no `_CODE` suffix for HEC protocol result-code knobs.

## Canonical Names And Original Evidence

Internal code uses one canonical decomposition even when external endpoints and configuration inputs have compatibility aliases.

```text
external route alias -> endpoint kind -> canonical handler -> canonical event/batch/sink objects
```

Examples:

- `/services/collector`, `/services/collector/event`, and `/services/collector/event/1.0` all map to endpoint kind `event`;
- `/services/collector/raw` and `/services/collector/raw/1.0` map to endpoint kind `raw`;
- future compatibility routes are route aliases, not separate parser or sink types.

Aliases may be accepted at the boundary. After loading or routing, behavior uses canonical names. Original spellings remain available as evidence.

Use this split:

```text
canonical_name = normalized internal behavior key
original_name  = external spelling observed at the boundary
```

Examples:

- `endpoint_kind = "raw"`, `route_alias = "/services/collector/raw/1.0"`;
- `token_source = "env"`, `original_name = "SPANK_HEC_TOKEN"`, `canonical_name = "HEC_TOKEN"`;
- `line_splitter = "scalar"`, `original_name = "body.split(|byte| *byte == b'\\n')"`.

Metrics that describe behavior use canonical names. Logs, traces, debug inspection, validation ledgers, compatibility reports, and benchmark records retain original names so a run can be reconstructed exactly.

## File Format

TOML file example:

```toml
[hec]
addr = "127.0.0.1:18088"
token = "dev-token"
capture = "/tmp/hec-events.jsonl"

[limits]
max_bytes = 1048576
max_decoded_bytes = 4194304
max_events = 100000
idle_timeout = "5s"
total_timeout = "30s"
gzip_buffer_bytes = 8192

[protocol]
success = 0
token_required = 2
invalid_authorization = 3
invalid_token = 4
no_data = 5
invalid_data_format = 6
server_busy = 9
event_field_required = 12
event_field_blank = 13
handling_indexed_fields = 15
health = 17
```

Defaults may be restated in TOML for explicit deployments. `--show-config` should print the effective merged config with secrets redacted and value sources available when source tracking is implemented.

## Core Parameters

| Parameter | TOML key | Env var | CLI flag | Default | Validation |
| --- | --- | --- | --- | --- | --- |
| Config file | none | `HEC_CONFIG` | `--config`, `-c` | none | readable file; valid TOML |
| Listen address | `hec.addr` | `HEC_ADDR` | `--addr` | `127.0.0.1:18088` | valid socket address; port in `1..=65535` |
| Token | `hec.token` | `HEC_TOKEN` | `--token` | `dev-token` | non-empty; no ASCII control characters; redacted in logs |
| Capture path | `hec.capture` | `HEC_CAPTURE` | `--capture` | none | non-empty path if set; parent directory must exist before bind if synchronous capture is enabled |
| Max wire bytes | `limits.max_bytes` | `HEC_MAX_BYTES` | `--max-bytes` | `1_048_576` | `1..=configured_absolute_max`; content-length and read cap |
| Max decoded bytes | `limits.max_decoded_bytes` | `HEC_MAX_DECODED_BYTES` | `--max-decoded-bytes` | `4_194_304` | `>= max_bytes` when gzip is enabled; bounded absolute maximum |
| Max events per request | `limits.max_events` | `HEC_MAX_EVENTS` | `--max-events` | `100_000` | `1..=configured_absolute_max` |
| Body idle timeout | `limits.idle_timeout` | `HEC_IDLE_TIMEOUT` | `--idle-timeout` | `5s` | valid duration; greater than zero; bounded upper value |
| Body total timeout | `limits.total_timeout` | `HEC_TOTAL_TIMEOUT` | `--total-timeout` | `30s` | valid duration; `>= idle_timeout`; bounded upper value |
| Gzip buffer bytes | `limits.gzip_buffer_bytes` | `HEC_GZIP_BUFFER_BYTES` | `--gzip-buffer-bytes` | `8_192` | power-of-two preferred; `512..=1_048_576` |
| Success result | `protocol.success` | `HEC_SUCCESS` | `--protocol-success` | `0` | valid HEC result code; unique unless explicitly aliased |
| Token required result | `protocol.token_required` | `HEC_TOKEN_REQUIRED` | `--protocol-token-required` | `2` | valid HEC result code |
| Invalid auth result | `protocol.invalid_authorization` | `HEC_INVALID_AUTHORIZATION` | `--protocol-invalid-authorization` | `3` | valid HEC result code |
| Invalid token result | `protocol.invalid_token` | `HEC_INVALID_TOKEN` | `--protocol-invalid-token` | `4` | valid HEC result code |
| No data result | `protocol.no_data` | `HEC_NO_DATA` | `--protocol-no-data` | `5` | valid HEC result code |
| Invalid data result | `protocol.invalid_data_format` | `HEC_INVALID_DATA_FORMAT` | `--protocol-invalid-data-format` | `6` | valid HEC result code |
| Server busy result | `protocol.server_busy` | `HEC_SERVER_BUSY` | `--protocol-server-busy` | `9` | valid HEC result code |
| Event field required result | `protocol.event_field_required` | `HEC_EVENT_FIELD_REQUIRED` | `--protocol-event-field-required` | `12` | valid HEC result code |
| Event field blank result | `protocol.event_field_blank` | `HEC_EVENT_FIELD_BLANK` | `--protocol-event-field-blank` | `13` | valid HEC result code |
| Indexed fields result | `protocol.handling_indexed_fields` | `HEC_HANDLING_INDEXED_FIELDS` | `--protocol-handling-indexed-fields` | `15` | valid HEC result code |
| Health result | `protocol.health` | `HEC_HEALTH` | `--protocol-health` | `17` | valid HEC result code |

Compatibility aliases such as `SPANK_HEC_TOKEN` may be accepted only at the provider boundary and must be reported as original source names, not as canonical field names.

## Growth Parameters

These settings are expected as the receiver gains owned listener construction, queueing, admission control, and parser strategy selection.

| Parameter | TOML key | Env var | CLI flag | Default candidate | Validation |
| --- | --- | --- | --- | --- | --- |
| Listen backlog | `network.listen_backlog` | `HEC_LISTEN_BACKLOG` | `--listen-backlog` | OS/Tokio default until measured | `1..=65535`; platform support noted |
| TCP receive buffer | `network.tcp_recv_buffer` | `HEC_TCP_RECV_BUFFER` | `--tcp-recv-buffer` | OS default | bytes range; platform may round |
| TCP send buffer | `network.tcp_send_buffer` | `HEC_TCP_SEND_BUFFER` | `--tcp-send-buffer` | OS default | bytes range; platform may round |
| TCP nodelay | `network.tcp_nodelay` | `HEC_TCP_NODELAY` | `--tcp-nodelay` | false unless benchmark says otherwise | boolean |
| Reuse address | `network.reuse_addr` | `HEC_REUSE_ADDR` | `--reuse-addr` | true | boolean; platform support noted |
| Reuse port | `network.reuse_port` | `HEC_REUSE_PORT` | `--reuse-port` | false | boolean; platform support noted |
| Runtime worker threads | `runtime.worker_threads` | `HEC_WORKER_THREADS` | `--worker-threads` | Tokio default | `1..=available_parallelism * configured_multiplier` |
| Queue depth | `queue.depth` | `HEC_QUEUE_DEPTH` | `--queue-depth` | benchmark-defined | `1..=configured_absolute_max` |
| Enqueue timeout | `queue.enqueue_timeout` | `HEC_ENQUEUE_TIMEOUT` | `--enqueue-timeout` | `0ms` or bounded wait | duration; valid for selected queue policy |
| Global connection limit | `admission.max_connections` | `HEC_MAX_CONNECTIONS` | `--max-connections` | benchmark-defined | `1..=configured_absolute_max` |
| Per-IP connection limit | `admission.max_connections_per_ip` | `HEC_MAX_CONNECTIONS_PER_IP` | `--max-connections-per-ip` | benchmark-defined | `1..=max_connections` |
| Per-IP range prefix v4 | `admission.ipv4_prefix_len` | `HEC_IPV4_PREFIX_LEN` | `--ipv4-prefix-len` | `32` | `0..=32` |
| Per-IP range prefix v6 | `admission.ipv6_prefix_len` | `HEC_IPV6_PREFIX_LEN` | `--ipv6-prefix-len` | `128` | `0..=128` |
| Header read timeout | `http.header_timeout` | `HEC_HEADER_TIMEOUT` | `--header-timeout` | benchmark-defined | duration greater than zero |
| Max header bytes | `http.max_header_bytes` | `HEC_MAX_HEADER_BYTES` | `--max-header-bytes` | Hyper/Axum default until owned | bytes range; must cover required HEC headers |
| Raw line max bytes | `limits.max_line_bytes` | `HEC_MAX_LINE_BYTES` | `--max-line-bytes` | benchmark-defined | `1..=max_decoded_bytes` |
| Raw line splitter | `raw.line_splitter` | `HEC_RAW_LINE_SPLITTER` | `--raw-line-splitter` | `scalar` | enum: `scalar`, `memchr`; SIMD only after agreement tests |
| Body overflow action | `policy.body_overflow` | `HEC_BODY_OVERFLOW` | `--body-overflow` | `reject` | enum from policy list |
| Decode overflow action | `policy.decode_overflow` | `HEC_DECODE_OVERFLOW` | `--decode-overflow` | `reject` | enum from policy list |
| Event overflow action | `policy.event_overflow` | `HEC_EVENT_OVERFLOW` | `--event-overflow` | `reject` | enum from policy list |
| Queue full action | `policy.queue_full` | `HEC_QUEUE_FULL` | `--queue-full` | `busy` | enum from policy list |
| Sink failure action | `policy.sink_failure` | `HEC_SINK_FAILURE` | `--sink-failure` | `busy` | enum from policy list |
| Global connection overflow action | `policy.connection_overflow` | `HEC_CONNECTION_OVERFLOW` | `--connection-overflow` | `reject` | enum from policy list |
| Per-IP overflow action | `policy.ip_overflow` | `HEC_IP_OVERFLOW` | `--ip-overflow` | `reject` | enum from policy list |

## Policy Values

Policy action values are enums, not booleans.

| Policy family | Initial values |
| --- | --- |
| Body/decode/event overflow | `reject`, `close`, `drain_then_reject` |
| Queue full | `busy`, `wait`, `drop_new`, `spill` |
| Sink failure | `busy`, `retry`, `spill`, `degrade` |
| Connection overflow | `reject`, `close_new`, `close_oldest_idle` |

Unsupported values fail at startup. A policy value must define HTTP response behavior, internal outcome, stats counters, log severity, and whether the connection remains usable.

## Command-Line Shape

Planned `clap` shape:

```text
hec-receiver [OPTIONS]

Options:
  -c, --config <PATH>
      Read TOML configuration file.

      --show-config
      Print effective merged configuration and exit.

      --check-config
      Validate configuration and exit.

      --addr <ADDR>
      Override hec.addr.

      --token <TOKEN>
      Override hec.token.

      --capture <PATH>
      Override hec.capture.
```

Do not add every advanced knob to help output immediately if it makes the interface unreadable. Group detailed knobs under clear prefixes and document them here.

## Validation Contract

Validation is a production safety boundary, not just type parsing.

Validation classes:

- **Format:** TOML syntax, socket address syntax, duration syntax, integer syntax, path syntax, UTF-8 where required, header-compatible token characters.
- **Known fields:** unknown TOML keys, CLI flags, env aliases, enum values, and policy actions fail fast unless explicitly marked compatibility-only.
- **Bounds:** byte limits, event counts, queue depths, timeouts, worker counts, port ranges, prefix lengths, and protocol result codes have minimum and maximum values.
- **Cross-field:** decoded limit must not be below wire limit when gzip is enabled; total timeout must not be below idle timeout; per-IP limit must not exceed global limit; line limit must not exceed decoded limit.
- **Canonical/industry:** HEC endpoint aliases, result codes, auth schemes, gzip names, source/sourcetype/index field names, IPv4/v6 prefix rules, and HTTP status mappings must use canonical names internally while preserving original evidence.
- **Safety:** secrets are non-empty and redacted; paths are checked before bind when startup depends on them; resource limits cannot be zero unless zero has a documented disabled meaning.
- **Reliability:** policies must define behavior under overflow, slow clients, sink failure, decode failure, and invalid request data.

Startup fails before binding sockets when validation fails. `--check-config` runs the same merge and validation path, reports all detected errors when practical, and exits without binding sockets.

## Error Detection, Logging, And Notification

This area is not fully resolved at implementation level yet; the production direction is fixed enough to prevent ad hoc behavior.

Central model:

- configuration errors: fail startup with a typed error, source field, original source name, and sanitized value description;
- startup errors: fail before serving when bind, capture path, runtime, or required sink initialization fails;
- request errors: return HEC-compatible JSON outcomes with centralized text/code/status mapping;
- internal errors: log with structured fields and increment counters;
- overload errors: use the configured policy and record whether data was rejected, drained, dropped, spilled, or accepted;
- sink errors: distinguish accepted, queued, captured, flushed, and durable states.

Status logging requirements:

- log effective startup config with secrets redacted;
- log library/runtime shape: binary version, Rust profile, Tokio worker count, listener address, config sources, selected splitter, selected sink;
- log one startup-ready event after successful bind and route installation;
- log shutdown reason and final counters;
- expose counters through a stats endpoint and optionally Prometheus later.

Notification requirements are intentionally narrow for now: process exit status, logs, stats endpoint, and benchmark ledgers. External notification sinks such as email, webhook, syslog, or OTel exporters are later integrations, not part of initial config.

## Timing, Performance, And Benchmark Recording

Configuration that affects timing or throughput must be benchmarkable and reproducible.

Required run metadata:

- git revision, binary version, build profile, OS, CPU model, core count, memory size;
- effective config with redacted secrets;
- selected listener, splitter, gzip, parser, queue, and sink settings;
- benchmark tool and version;
- request corpus path and size;
- start/end timestamps and duration source;
- bytes accepted, bytes rejected, events accepted, events written, failures, latency percentiles when available.

Timing points should be captured separately when the code path exists:

```text
accept -> headers/auth -> body read -> gzip decode -> parse/framing -> enqueue -> sink write -> flush/durable state
```

Benchmark output should be append-only JSONL or CSV with enough fields to reproduce the run. Human summaries are derived from this ledger, not the only record.

## Central Definitions And Message Text

Protocol result definitions, HTTP status mappings, user-facing message text, policy names, and config key names should be centralized where suitable.

Target call-site style:

```rust
return Err(outcomes.invalid_authorization().with_reason(AuthReason::MalformedHeader));
return Err(config_errors.invalid_value(field::LIMITS_MAX_BYTES, value).with_bound("1..=absolute_max"));
stats.count(counter::REQUESTS_REJECTED, labels::endpoint(endpoint_kind));
```

The point is not ceremony. The point is tight representation with clear call-site invocation: callers name the domain condition; centralized definitions decide exact text, code, status, labels, and redaction behavior.

Avoid scattering string literals such as `"Invalid data format"`, `"server_busy"`, `"HEC_MAX_BYTES"`, or `"/services/collector/raw"` through handlers and tests. Tests may assert centralized values, but should not become the only place where protocol text is defined.

## Tests Required

Every configurable field should have a round-trip test:

1. default value exists;
2. TOML key overrides default;
3. CLI flag overrides TOML;
4. env var overrides CLI;
5. `--show-config` prints the effective value with secrets redacted;
6. `--check-config` accepts valid configurations and rejects invalid ones without binding sockets;
7. invalid format, unknown value, out-of-bounds value, and cross-field violation fail cleanly;
8. original source names and canonical field names are both available for diagnostics.

Config tests should include generated table coverage so adding a field without default, validation, source mapping, and documentation becomes visible.
