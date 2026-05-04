# InfraHEC — HECpoc Infrastructure Spine

`InfraHEC.md` concentrates the cross-cutting infrastructure plan for the HECpoc Rust implementation: runtime, configuration, errors, messages, logging, metrics, lifecycle, validation, benchmarking, security posture, and operational packaging. It adopts the best layout ideas from:

- `/Users/walter/Work/Spank/infra/Infrastructure.md` — functional layers, scale regimes, protocol selection, and operations framing;
- `/Users/walter/Work/Spank/spank-py/Infra.md` — problem/benefit/requirements/architecture/decision sections, call-site conventions, metrics/health, security posture, and validation survey style;
- `/Users/walter/Work/Spank/spank-rs/research/Infrust.md` — Rust-specific mandates for `tracing`, metrics, error taxonomy, `figment`, lifecycle, Tokio runtime, CLI, health, testing, and build tooling;
- current HECpoc documents: `HECpoc.md`, `Config.md`, `ErrorMessaging.md`, `Stack.md`, and `PerfIntake.md`.

This is the current infrastructure reference for implementation. Specialized documents may remain as detail ledgers, but new cross-cutting implementation decisions should land here first.

---

## 1. Purpose And Scope

HECpoc is a focused Rust implementation of a Splunk HEC-compatible receiver for local testing, validation, and later production-quality ingest experiments. Infrastructure means the code and operational machinery that every feature depends on:

- process startup and shutdown;
- configuration and validation;
- HTTP/Tokio runtime behavior;
- errors, HEC outcomes, and message text;
- logs, metrics, health, readiness, and run ledgers;
- request buffering, resource limits, backpressure, and resilience;
- test harnesses, benchmarks, fixtures, and external compatibility checks;
- build, packaging, and service operation.

Near-term product scope:

```text
HEC HTTP input -> auth/body/decode/parse -> event batch -> capture sink -> inspection/validation
```

Explicitly outside the initial infrastructure target:

- hot configuration reload;
- distributed notification sinks;
- full Splunk search implementation;
- per-log-type database partitioning during ingest;
- production TLS lifecycle;
- full indexer ACK durability semantics;
- specialized parser/index/search acceleration except as benchmarked follow-up.

---

## 2. Adopted Document Pattern

The three reviewed infrastructure documents contribute different strengths.

| Source | Best Part To Adopt | HECpoc Adaptation |
| --- | --- | --- |
| `infra/Infrastructure.md` | Functional layers and scale regimes | Define HEC ingest layers from network edge to sink, with small/mid/large operating modes |
| `spank-py/Infra.md` | Problem, requirements, architecture, decisions, call-site conventions | Each subsystem section names benefits, requirements, implementation shape, and call-site discipline |
| `spank-rs/research/Infrust.md` | Rust library mandates and operational patterns | Use `tracing`, `clap`, `figment`, `thiserror`, Tokio Builder, signal handling, nextest/fuzz/bench tooling |
| `HECpoc.md` | Current product scope and implementation sequence | Preserve scope, capability bundles, and first work sequence |
| `Config.md` | Config surface and validation categories | Fold into one infrastructure-wide config contract |
| `ErrorMessaging.md` | Error/outcome/message subsystem | Fold into error, logging, stats, and call-site sections |
| `Stack.md` | HTTP/Tokio/Axum, Tower avoidance, backpressure, byte stages | Fold into runtime, HTTP stack, body processing, and resilience sections |
| `PerfIntake.md` | Performance caution and benchmark orientation | Use benchmark evidence to admit optimizations |

Preferred section cadence:

1. problem and benefit;
2. requirements;
3. architecture or implementation shape;
4. call-site conventions where relevant;
5. validation and acceptance;
6. open decisions only when the decision is genuinely not yet made.

History and abandoned approaches do not belong in the main flow. They can be noted in `History.md` when needed.

---

## 3. Functional Layers For HECpoc

Borrowing the layered discipline from the broad infrastructure document, HECpoc uses functional layers rather than vendor/product layers.

| Layer | HECpoc Layer | Responsibility | Initial Implementation |
| --- | --- | --- | --- |
| 1 | Network edge | TCP accept, peer identity, connection counts, receive buffers | Tokio listener through Axum initially; owned accept instrumentation later |
| 2 | HTTP envelope | method/path/header/body framing, content-length, timeouts | Axum adapter with explicit body reader |
| 3 | HEC protocol | route aliases, auth, gzip, raw/event endpoints, HEC outcomes | `src/hec_receiver` protocol modules |
| 4 | Event formation | JSON envelopes, raw line framing, metadata, event batch | `HecEvent`, `EventBatch`, `LineSplitter` |
| 5 | Handoff and sink | bounded queue, capture sink, commit state | direct sink now; bounded queue next |
| 6 | Inspection and validation | readback, stats, process tests, compatibility ledgers | stats route, capture files, fixtures, benchmark ledgers |
| 7 | Operations | config, logs, lifecycle, packaging, health, security posture | `InfraHEC.md` mandates and implementation work items |

Layering rule:

```text
lower layer exposes facts and bounded data; upper layer decides semantics
```

Examples:

- TCP does not decide HEC auth.
- Raw line splitting does not decide tokenization/search semantics.
- Gzip decode reports decode facts and limits; HEC outcome mapping decides client response.
- Sink commit state reports what happened; response policy decides whether success means queued, captured, flushed, or durable.

---

## 4. Operating Modes And Scale Regimes

The broad infrastructure document distinguishes small, mid, and large scale. HECpoc should use the same idea to avoid either toy-only design or premature enterprise machinery.

| Mode | Intended User | Infrastructure Shape | Success Criteria |
| --- | --- | --- | --- |
| Local fixture | developer/CI on one machine | one process, local token, capture file, stats endpoint | deterministic request/response and readback |
| Compatibility lab | developer comparing clients | local Splunk, Vector, curl/`oha`/`wrk`, fixture ledgers | explain differences by exact request, response, and capture evidence |
| Performance lab | systems developer | pinned configs, benchmark ledger, host/kernel stats, controlled corpora | bytes/sec, events/sec, latency, CPU/memory evidence by stage |
| Production candidate | operator | service unit/container, structured logs, health/readiness, bounded resources | predictable startup/shutdown, config validation, overload policy, no crash on hostile input |

Do not add production machinery before a mode needs it. Do define interfaces so production machinery has somewhere clean to attach.

---

## 5. Mainstay Rust Infrastructure Choices

These are the project mainstays unless a later benchmark or protocol proof disproves them.

| Area | Choice | Reason |
| --- | --- | --- |
| Language | Rust-only | Fresh start; avoids Python concurrency limitations and mixed-language uncertainty |
| Async runtime | Tokio multi-thread | mature ecosystem, Axum/Hyper compatibility, signal/time/fs support |
| HTTP framework | Axum as thin adapter | practical routing and response conversion while keeping protocol logic ours |
| Direct HTTP fallback | Hyper + `hyper-util` | escape hatch if Axum blocks exact protocol or accept-loop control |
| Configuration | `clap` + `figment` + `serde` + TOML | typed layered config without hand-coded merge sprawl |
| Error definitions | `thiserror` or local typed enums | classify failures without stringly exceptions |
| Binary-level errors | `anyhow` acceptable only at top-level binary boundary | contextual startup failure without leaking into core protocol code |
| Logging | `tracing` + `tracing-subscriber` JSON to stderr | standard Rust structured logging path |
| Metrics | in-process counters first; `metrics`/Prometheus later | avoid premature external interface while preserving names |
| Testing | `cargo test` now; `nextest` later | current simplicity, production parallelism later |
| Benchmarks | `criterion` for micro, ledgered external tools for system | separate parser/body/sink timings from end-to-end load |
| Fuzz/property | `proptest`, `cargo-fuzz` later | high value for parsers and body/framing edge cases |

Tower middleware rule: avoid Tower for protocol-critical auth, gzip, body limits, and timeouts until our exact HEC behavior is defined and tested. Axum itself remains acceptable as an HTTP adapter.

---

## 6. Repository And Module Shape

Initial single-crate shape:

```text
HECpoc/
  Cargo.toml
  HECpoc.md
  InfraHEC.md
  Config.md
  ErrorMessaging.md
  Stack.md
  History.md
  scripts/
  fixtures/
    requests/
    configs/
    logs/
    ledgers/
  src/
    main.rs
    hec_receiver/
      mod.rs
      app.rs
      auth.rs
      body.rs
      config.rs
      error.rs
      event.rs
      handler.rs
      health.rs
      line_splitter.rs
      messages.rs
      outcome.rs
      parse_event.rs
      parse_raw.rs
      protocol.rs
      sink.rs
      stats.rs
```

Crate split rule:

- Stay one crate until a component is internally complete and separately reusable.
- Candidate future splits: protocol parser, collector, storage, benchmark harness.
- Do not split merely to imitate old workspace structure.

Naming rule:

- Use event/source/context/sink/commit vocabulary.
- Avoid ambiguous ingest names such as `Sender` for a sink or `Row` for a protocol event.
- Preserve original external names as evidence fields, not behavior keys.

---

## 7. Configuration Management

### 7.1 Problem And Benefit

Configuration is a production infrastructure subsystem, not incidental argument parsing. It must support deterministic local runs, operator-edited files, CI/container overrides, and reproducible benchmark ledgers.

Benefits:

- one typed model;
- one precedence rule;
- one validation path;
- clear error messages before binding sockets;
- redacted effective config output;
- no hand-coded source merge drift.

### 7.2 Source Chain

Configuration sources:

1. compiled defaults;
2. TOML config file;
3. command-line flags;
4. environment variables.

Precedence:

```text
compiled defaults < TOML file < command line < environment
```

Implementation:

- `clap` parses CLI and identifies config path plus CLI overrides;
- `figment` composes providers in precedence order;
- `serde` extracts typed structs;
- explicit `validate()` or `validator` checks values after extraction;
- effective config is logged with secrets redacted.

Hot reload is not a foreseeable goal. The initial configuration is frozen after startup validation.

### 7.3 Naming

| Source | Form | Example |
| --- | --- | --- |
| Rust field | snake_case | `max_decoded_bytes` |
| TOML key | section + snake_case | `limits.max_decoded_bytes` |
| CLI flag | kebab-case | `--max-decoded-bytes` |
| Env var | upper snake with `HEC_` prefix | `HEC_MAX_DECODED_BYTES` |

Avoid:

- `POC` in config names;
- `_BODY` when the actual concept is wire bytes or decoded bytes;
- `_MILLIS`; use duration strings;
- `_CODE`; protocol result settings are named by outcome.

### 7.4 Core Parameters

| Parameter | TOML key | Env var | CLI flag | Default | Validation |
| --- | --- | --- | --- | --- | --- |
| Config file | none | `HEC_CONFIG` | `--config`, `-c` | none | readable TOML |
| Listen address | `hec.addr` | `HEC_ADDR` | `--addr` | `127.0.0.1:18088` | valid socket address |
| Token | `hec.token` | `HEC_TOKEN` | `--token` | `dev-token` | non-empty; redacted |
| Capture path | `hec.capture` | `HEC_CAPTURE` | `--capture` | none | parent exists when required |
| Max wire bytes | `limits.max_bytes` | `HEC_MAX_BYTES` | `--max-bytes` | `1_048_576` | positive bounded bytes |
| Max decoded bytes | `limits.max_decoded_bytes` | `HEC_MAX_DECODED_BYTES` | `--max-decoded-bytes` | `4_194_304` | `>= max_bytes` when gzip enabled |
| Max events per request | `limits.max_events` | `HEC_MAX_EVENTS` | `--max-events` | `100_000` | positive bounded count |
| Body idle timeout | `limits.idle_timeout` | `HEC_IDLE_TIMEOUT` | `--idle-timeout` | `5s` | positive duration |
| Body total timeout | `limits.total_timeout` | `HEC_TOTAL_TIMEOUT` | `--total-timeout` | `30s` | `>= idle_timeout` |
| Gzip buffer bytes | `limits.gzip_buffer_bytes` | `HEC_GZIP_BUFFER_BYTES` | `--gzip-buffer-bytes` | `8_192` | bounded bytes |

Protocol result settings remain configurable but centralized through outcome definitions:

| Outcome | TOML key | Env var | CLI flag | Default code |
| --- | --- | --- | --- | --- |
| success | `protocol.success` | `HEC_SUCCESS` | `--protocol-success` | `0` |
| token required | `protocol.token_required` | `HEC_TOKEN_REQUIRED` | `--protocol-token-required` | `2` |
| invalid authorization | `protocol.invalid_authorization` | `HEC_INVALID_AUTHORIZATION` | `--protocol-invalid-authorization` | `3` |
| invalid token | `protocol.invalid_token` | `HEC_INVALID_TOKEN` | `--protocol-invalid-token` | `4` |
| no data | `protocol.no_data` | `HEC_NO_DATA` | `--protocol-no-data` | `5` |
| invalid data format | `protocol.invalid_data_format` | `HEC_INVALID_DATA_FORMAT` | `--protocol-invalid-data-format` | `6` |
| server busy | `protocol.server_busy` | `HEC_SERVER_BUSY` | `--protocol-server-busy` | `9` |
| event field required | `protocol.event_field_required` | `HEC_EVENT_FIELD_REQUIRED` | `--protocol-event-field-required` | `12` |
| event field blank | `protocol.event_field_blank` | `HEC_EVENT_FIELD_BLANK` | `--protocol-event-field-blank` | `13` |
| handling indexed fields | `protocol.handling_indexed_fields` | `HEC_HANDLING_INDEXED_FIELDS` | `--protocol-handling-indexed-fields` | `15` |
| health | `protocol.health` | `HEC_HEALTH` | `--protocol-health` | `17` |

### 7.5 Growth Parameters

Growth parameters enter only with matching implementation and validation:

- listener backlog, receive/send buffers, nodelay, reuseaddr, reuseport;
- runtime worker threads, blocking thread cap, thread names;
- queue depth and enqueue timeout;
- global and per-IP connection limits;
- IPv4/v6 prefix grouping;
- HTTP header timeout and max header bytes;
- raw line max bytes and raw line splitter strategy;
- overflow policies for body, decode, event, queue, sink, and connection limits.

Policy values are enums. Initial families:

| Policy family | Initial values |
| --- | --- |
| body/decode/event overflow | `reject`, `close`, `drain_then_reject` |
| queue full | `busy`, `wait`, `drop_new`, `spill` |
| sink failure | `busy`, `retry`, `spill`, `degrade` |
| connection overflow | `reject`, `close_new`, `close_oldest_idle` |

Each policy must define HTTP response, HEC outcome, internal state, stats counter, log severity, and connection usability.

### 7.6 CLI Shape

Initial command:

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

Output discipline:

- primary output such as `--show-config` goes to stdout;
- diagnostics go to stderr;
- invalid config exits before bind;
- use sysexits-like meaning where practical: usage/config/tempfail distinctions.

### 7.7 Config Validation

Validation classes:

- **format:** TOML syntax, socket address, duration, integer, path, UTF-8 where required;
- **known values:** TOML keys, CLI flags, env aliases, enum values, policy values;
- **bounds:** bytes, counts, queue depths, timeouts, worker counts, port ranges, prefix lengths, protocol codes;
- **cross-field:** decoded bytes >= wire bytes, total timeout >= idle timeout, per-IP <= global, line max <= decoded max;
- **canonical/industry:** HEC route aliases, result codes, auth schemes, gzip names, source/sourcetype/index names, IPv4/v6 prefixes;
- **safety:** redacted secrets, non-zero resource limits unless explicitly disabled, path checks before bind;
- **reliability:** overflow, slow client, sink failure, decode failure, invalid request behavior defined.

Acceptance:

- every field has default, TOML override, CLI override, env override, validation test, `--show-config` coverage, and `--check-config` coverage;
- diagnostics include canonical field name and original source name;
- secrets are redacted.

---

## 8. Error, Outcome, Message, And Status Infrastructure

### 8.1 Problem And Benefit

The receiver must distinguish internal errors from HEC client-visible outcomes. Without centralization, handlers scatter string literals, HTTP statuses, HEC result codes, log labels, and stats reasons. That becomes impossible to compare against Splunk/Vector and hard to validate.

Benefits:

- stable HEC response bodies;
- typed internal failure classes;
- compact call sites;
- centralized redaction;
- consistent counters and log fields;
- easier oracle comparison.

### 8.2 Module Boundaries

```text
src/hec_receiver/
  error.rs        internal startup/config/runtime error types
  outcome.rs      HEC client-visible outcomes and response conversion
  messages.rs     centralized text, field names, labels, redaction helpers
  stats.rs        counters and snapshots
```

Semantic boundary:

- `error.rs` classifies failures;
- `outcome.rs` defines client responses;
- `messages.rs` holds centralized strings and symbols;
- HTTP handlers convert errors to outcomes at the adapter edge;
- stats/logging happen through the same mapping path.

### 8.3 Error Classes

| Class | Examples | Client exposure |
| --- | --- | --- |
| `Config` | invalid TOML, unknown field, bad env, invalid bounds | startup diagnostic only |
| `Startup` | bind failure, capture path failure, runtime construction | startup diagnostic only |
| `Auth` | missing token, malformed header, bad scheme, invalid token | HEC auth outcomes |
| `Body` | content-length too large, timeout, read error | HEC/body policy outcome |
| `Decode` | gzip invalid, decoded limit exceeded, unsupported encoding | invalid data or unsupported encoding policy |
| `Parse` | invalid JSON, missing/blank event, invalid raw framing | HEC parser outcomes |
| `Queue` | full queue, enqueue timeout, closed worker | server busy or configured queue policy |
| `Sink` | write, flush, durable commit failure | depends on commit state |
| `Shutdown` | signal, graceful timeout, worker join failure | log/status only |
| `Internal` | invariant violation, unexpected join error | generic server busy or startup failure; detailed log only |

Top-level boundary:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),
    #[error("startup error: {0}")]
    Startup(#[from] StartupError),
    #[error("request error: {0}")]
    Request(#[from] RequestError),
    #[error("sink error: {0}")]
    Sink(#[from] SinkError),
}
```

The actual enum may start smaller, but subsystem boundaries should not collapse into `Box<dyn Error>`.

### 8.4 HEC Outcomes

Central type:

```rust
pub struct HecOutcome {
    pub status: StatusCode,
    pub text: &'static str,
    pub code: u16,
    pub metadata: Option<HecOutcomeMetadata>,
}
```

Initial constructors:

| Constructor | Text | Code | HTTP status | Notes |
| --- | --- | --- | --- | --- |
| `success()` | `Success` | `0` | `200` | accepted according to current sink mode |
| `success_with_ack_id(id)` | `Success` | `0` | `200` | later ACK support |
| `token_required()` | `Token required` | `2` | `401` | missing token |
| `invalid_authorization()` | `Invalid authorization` | `3` | `401` | malformed header or unsupported scheme |
| `invalid_token()` | `Invalid token` | `4` | measured | unknown token |
| `no_data()` | `No data` | `5` | `400` | empty body or no events |
| `invalid_data_format()` | `Invalid data format` | `6` | `400` | JSON/raw/gzip parse class |
| `server_busy()` | `Server is busy` | `9` | `503` | queue/sink/backpressure |
| `event_field_required(index)` | `Event field is required` | `12` | `400` | metadata carries index |
| `event_field_blank(index)` | `Event field cannot be blank` | `13` | `400` | metadata carries index |
| `handling_indexed_fields()` | `Error in handling indexed fields` | `15` | `400` | fields policy |
| `health()` | `HEC is healthy` | `17` | `200` | health endpoint |
| `ack_disabled()` | TBD | TBD | TBD | verify against Splunk/Vector |
| `unsupported_encoding()` | TBD | TBD | `415` or invalid data | verify against Splunk/Vector |
| `body_too_large()` | TBD | TBD | `413` or HEC error | verify against Splunk/Vector |

Response body shape:

```json
{"text":"Invalid data format","code":6}
```

Client-visible text is fixed. Never include parser exceptions, token values, file paths, Rust error text, or body excerpts in HEC responses.

### 8.5 Mapping Rules

Mapping happens once at the HTTP adapter edge:

```text
AuthError -> HecOutcome
BodyError -> HecOutcome
DecodeError -> HecOutcome
ParseError -> HecOutcome
QueueError -> HecOutcome
SinkError + SinkCommitState -> HecOutcome
```

Example policy:

| Internal condition | HEC outcome | Counter reason | Log severity |
| --- | --- | --- | --- |
| missing auth token | `token_required()` | `token_required` | `warn` |
| malformed auth header | `invalid_authorization()` | `invalid_authorization` | `warn` |
| invalid token | `invalid_token()` | `invalid_token` | `warn` |
| empty body | `no_data()` | `no_data` | `info` |
| invalid JSON | `invalid_data_format()` | `invalid_json` | `info`/`warn` |
| gzip decode failed | `invalid_data_format()` | `gzip_decode_failed` | `warn` |
| queue full | `server_busy()` | `queue_full` | `warn` |
| sink unavailable before acceptance | `server_busy()` | `sink_unavailable` | `error` |

Every mapped outcome increments stats and emits structured log fields.

### 8.6 Central Definitions And Call Sites

Centralized groups:

- endpoint route aliases;
- endpoint kinds;
- HEC result text;
- HEC default codes;
- HTTP status mappings;
- config field names;
- env var names;
- policy names and values;
- counter names;
- log field names;
- redaction rules.

Preferred call-site style:

```rust
return Err(AuthError::malformed_header(AuthHeaderProblem::NonText));
return Err(ParseError::event_field_required().with_event_index(index));
let outcome = outcomes::invalid_authorization().with_reason(AuthReason::MalformedHeader);
stats.count(counter::REQUESTS_REJECTED, labels::reason(reason::INVALID_AUTHORIZATION));
```

Avoid:

```rust
return (StatusCode::BAD_REQUEST, Json(json!({"text":"Invalid data format","code":6})));
```

### 8.7 Redaction

Never log or return:

- token values;
- raw authorization header values;
- full request bodies;
- decompressed bodies;
- full malformed JSON payloads;
- arbitrary untrusted filesystem paths.

Allowed:

- token presence/absence;
- token id or hash once token registry exists;
- route alias;
- endpoint kind;
- auth scheme spelling;
- byte lengths;
- parse class;
- safe byte offset;
- event index;
- canonical source/sourcetype/index;
- original operator-provided evidence when safe.

---

## 9. Structured Logging And Observability

### 9.1 Stack

Use `tracing` plus `tracing-subscriber` with JSON output to stderr.

Initial logging config:

- `EnvFilter` or config field controls level;
- JSON formatter for structured logs;
- stderr only; supervisor captures persistence;
- no per-event log calls on hot ingest paths;
- per-request outcome log is acceptable at controlled level;
- warnings/errors for malformed input, overload, and sink failures.

Later:

- compile-time `release_max_level_info` for production builds;
- `tracing-error` if error context needs to be carried to boundaries;
- OTel exporter only when distributed tracing becomes a product feature.

### 9.2 Startup Logs

Fields:

- `event="startup"`;
- `version`;
- `git_revision` when available;
- `profile`;
- `addr`;
- `config_sources`;
- `sink_kind`;
- `line_splitter`;
- `tokio_workers`;
- redacted effective config summary.

Ready event:

```text
config parsed -> tracing ready -> runtime built -> listener bound -> routes installed -> startup_ready
```

### 9.3 Request Outcome Logs

Fields:

- `event="request_outcome"`;
- `request_id` when available;
- `peer_addr`;
- `method`;
- `route_alias`;
- `endpoint_kind`;
- `status`;
- `hec_code`;
- `outcome_kind`;
- `reason`;
- `wire_bytes`;
- `decoded_bytes`;
- `event_count`;
- `duration_us`;
- `sink_commit_state` when reached.

### 9.4 Shutdown Logs

Fields:

- `event="shutdown"`;
- `reason`;
- `uptime_ms`;
- final counters;
- worker join status;
- flush status.

### 9.5 Health And Readiness

Initial same-port stats route exists. Production direction separates observability from data plane when needed.

Minimum endpoints:

| Path | Purpose | Initial status |
| --- | --- | --- |
| `/hec/stats` | current JSON counters | implemented/current shape |
| `/healthz` | liveness | planned |
| `/readyz` | readiness | planned |
| `/metrics` | Prometheus exposition | later |
| `/metrics/json` | compact JSON metrics | later |

Readiness should reflect subsystem state: listener, config, sink, queue, and shutdown/drain state.

---

## 10. Metrics And Counters

Initial counters use canonical names and bounded labels.

Required counters:

- `requests_total`;
- `requests_accepted_total`;
- `requests_rejected_total` by reason;
- `bytes_wire_total`;
- `bytes_decoded_total`;
- `events_accepted_total`;
- `events_written_total`;
- `body_read_errors_total`;
- `gzip_errors_total`;
- `parse_errors_total`;
- `auth_errors_total`;
- `queue_full_total`;
- `sink_failures_total`;
- `connections_current`;
- `connections_accepted_total`;
- `connections_closed_total`;
- `connections_rejected_total`;
- `connections_max`.

Counter arithmetic:

```text
connections_current = connections_accepted_total - connections_closed_total - active_rejected_after_accept_adjustments
connections_max = high_watermark(connections_current)
```

Labels:

- endpoint kind;
- reason enum;
- sink kind;
- commit state;
- parser/splitter kind.

Avoid labels containing token values, raw source paths, raw messages, or arbitrary error text.

Prometheus later uses the same counter names. The in-process stats route should not invent a separate vocabulary.

---

## 11. Process Lifecycle And Signals

### 11.1 Startup Sequence

```text
parse CLI -> compose config -> validate -> init tracing -> install panic hook -> build runtime -> build sink -> bind listener -> install routes -> log ready -> serve
```

Startup fails before bind for invalid config, missing required files/directories, invalid runtime settings, or required sink initialization failure.

### 11.2 Signals

Use Tokio signal handling:

- SIGINT: developer interrupt, graceful shutdown;
- SIGTERM: service/container stop, graceful shutdown;
- SIGHUP: no hot reload for foreseeable future; either log unsupported or use supervisor-level restart semantics later.

Use a root cancellation token when subsystems exist. Each subsystem receives a child token.

### 11.3 Graceful Shutdown

Shutdown phases:

1. stop accepting new work;
2. allow in-flight requests to finish within timeout;
3. stop queue intake;
4. drain queue according to policy;
5. flush sink if configured;
6. log final counters;
7. exit with appropriate status.

If Axum's basic graceful shutdown lacks required timeout/control, adopt `axum-server` or an owned Hyper accept loop.

### 11.4 Panic Policy

Production candidate policy:

- install panic hook that logs panic location and backtrace through `tracing`;
- release profile may use `panic = "abort"` after tests and service supervision are established;
- no attempt to recover from unknown panics in request handlers without explicit isolation.

---

## 12. Tokio Runtime And Concurrency

Use a Tokio multi-thread runtime.

Initial runtime may use `#[tokio::main]`; production direction is explicit `tokio::runtime::Builder` so config can drive worker counts and thread names.

Runtime knobs:

- `runtime.worker_threads`;
- `runtime.max_blocking_threads` later;
- `runtime.thread_stack_size` only if measurement warrants;
- thread name prefix such as `hec-worker`.

Concurrency design:

```text
network concurrency: Tokio/Axum request tasks
sink concurrency: bounded queue + one or more sink workers
CPU-heavy parsing/indexing: explicit worker boundary later, not casual tokio::spawn
blocking filesystem: isolated through sink workers or spawn_blocking/tokio::fs with measurement
```

No shared mutable state unless message passing is worse. Use `Arc` for shared immutable/config/state handles; use locks only for small stats/snapshots until contention is measured.

---

## 13. HTTP Stack And HEC Request Pipeline

### 13.1 Stack Decision

Start with Tokio plus Axum. Do not use Tower middleware for protocol-critical behavior initially.

Axum responsibilities:

- route request to handler;
- expose method, path, headers, body stream;
- convert final `HecOutcome` to HTTP response.

HECpoc responsibilities:

- auth semantics;
- gzip decode semantics;
- body limits and timeouts;
- raw/event parse semantics;
- HEC outcome mapping;
- stats/logging;
- sink state.

### 13.2 Request Pipeline

```text
TCP accept
  -> HTTP method/path routing
  -> route alias -> endpoint kind
  -> header validation/auth
  -> bounded body read
  -> optional gzip decode with decoded cap
  -> endpoint parse/framing
  -> EventBatch
  -> enqueue or direct sink
  -> SinkCommit
  -> HecOutcome
  -> stats/log
```

### 13.3 Endpoint Semantics

Initial endpoint kinds:

| Route aliases | Endpoint kind | Notes |
| --- | --- | --- |
| `/services/collector`, `/services/collector/event`, `/services/collector/event/1.0` | `event` | JSON envelopes, possible concatenated objects |
| `/services/collector/raw`, `/services/collector/raw/1.0` | `raw` | line-framed raw events |
| `/services/collector/health`, `/services/collector/health/1.0` | `health` | HEC health semantics |
| `/services/collector/ack` | `ack` | disabled/deferred until durable state exists |

Canonical endpoint kind drives behavior. Route alias remains evidence.

### 13.4 Auth

Requirements:

- accept expected HEC auth form;
- distinguish missing token, malformed header, bad scheme, invalid token;
- reject non-text or control-character header values safely;
- never log token values;
- map to centralized outcomes.

### 13.5 Body, Gzip, Limits, And Timeouts

Body reader owns:

- max content length / max wire bytes;
- idle timeout between body chunks;
- total timeout for body read;
- error classification for read failures.

Gzip decode owns:

- supported encoding detection;
- gzip decode errors;
- decoded byte cap;
- buffer size;
- no decompressor exception text in client responses.

### 13.6 Raw Byte And Character Semantics

Stages:

| Stage | CRLF | NUL | Controls | Non-ASCII / invalid UTF-8 |
| --- | --- | --- | --- | --- |
| TCP/body bytes | data | data | data | data |
| HTTP headers | header syntax rules | invalid in normal headers | generally invalid | `to_str()` rejects non-visible/non-ASCII |
| gzip decode | data after decode | data | data | data |
| raw splitter | split LF, trim one preceding CR | data | LF delimiter, CR trim only before LF | current lossy text; future bytes plus derived text |
| JSON parser | JSON whitespace/string rules | escaped only | unescaped invalid | JSON must be UTF-8 |
| tokenizer/indexer | later policy | later policy | later policy | later policy |

Raw splitter tests must cover LF, CRLF, lone CR, embedded CR, final no-LF, empty lines, NUL, controls, valid multibyte UTF-8, invalid UTF-8, and long lines.

---

## 14. Sink, Store, Queue, And Inspection

Initial sink is capture-oriented, not final product storage.

Commit states:

| State | Meaning | Can HTTP success mean this? |
| --- | --- | --- |
| parsed | syntax accepted | no |
| accepted | valid events formed | no by itself |
| queued | entered bounded handoff | yes in async mode if documented |
| captured | sink write returned | yes in synchronous capture mode |
| flushed | userspace writer flushed | yes only if mode waits |
| durable | fsync/DB durable commit complete | yes for future ACK/durable mode |

Initial order:

1. direct capture sink with explicit commit state;
2. bounded queue and single sink worker;
3. queue-full policy;
4. inspection helper over capture files;
5. hot bucket/segment format only after replay/durability needs are real.

Do not partition ingest by host/file/log type into separate databases during initial ingest. Preserve logical fields, then resort/coalesce for search preparation later.

---

## 15. Backpressure, Buffering, And Resilience

Layered path:

```text
client userspace -> client kernel -> network -> server kernel -> Tokio/Hyper/Axum -> bounded body -> decode -> parse/framing -> queue -> sink -> filesystem/page cache
```

Controls by layer:

| Layer | Initial controls | Future controls |
| --- | --- | --- |
| kernel/listener | OS defaults | backlog, recv/send buffers, reuseaddr/reuseport |
| HTTP body | wire cap, idle timeout, total timeout | streaming parser, header timeout |
| gzip | decoded cap | decompression ratio policy |
| parser | event cap | line cap, field cap, partial success policy |
| queue | direct sink now | bounded depth, enqueue timeout, queue-full policy |
| sink | capture path | flush/durable policy, spill/retry/degrade |
| connection | stats later | global/per-IP limits, idle culling |

Promoted resilience behavior:

- bounded bytes;
- bounded decoded bytes;
- bounded events;
- bounded time;
- bounded queue;
- bounded connection count later;
- no crash on malformed input;
- visible outcomes and counters.

DoS posture starts with resource caps and explicit rejection. Sophisticated mitigation enters only after measurement.

---

## 16. Security Posture

Initial security scope:

- token auth only;
- no token values in logs or responses;
- strict header parsing;
- body/gzip limits to resist memory expansion;
- parser must not panic on malformed bytes or JSON;
- request logs avoid raw body content;
- local bind default until deliberate exposure;
- external TLS/encryption postponed but route design must not prevent it.

Later hardening:

- TLS/rustls;
- token registry with IDs and allowed indexes;
- per-IP admission;
- rate limits;
- separate observability listener;
- systemd hardening;
- fuzz corpus for auth/body/gzip/parser.

---

## 17. Validation And Test Harness

### 17.1 Test Layers

| Layer | Tooling | Purpose |
| --- | --- | --- |
| Unit | `cargo test` / `#[tokio::test]` | auth, body, gzip, parser, outcome, config validation |
| Handler | Axum request/response tests | HTTP status/body/header behavior without socket |
| Process | binary + curl/fixtures | startup, bind, real request flow, capture files |
| Compatibility | Splunk Enterprise, Vector | compare external behavior |
| Corpus | local tutorial/prod logs, LogHub, attack corpora | realistic input variation |
| Hostile | radamsa, SecLists, PayloadsAllTheThings, slow clients | malformed/DoS scenarios |
| Benchmark | `oha`, `wrk`, `ab`, custom scripts | throughput, latency, concurrency |

### 17.2 Fixture Layout

```text
fixtures/
  requests/
    event_valid.json
    event_missing_event.json
    event_blank_event.json
    raw_crlf.txt
    raw_nul.bin
    gzip_valid.bin
    gzip_invalid.bin
  configs/
    minimal.toml
    invalid_unknown_field.toml
    invalid_bounds.toml
  logs/
    README.md
  ledgers/
    README.md
```

Large corpora are referenced by absolute path or manifest, not copied into the repo by default.

### 17.3 Config Tests

Every field requires:

1. default exists;
2. TOML overrides default;
3. CLI overrides TOML;
4. env overrides CLI;
5. invalid format fails;
6. invalid value fails;
7. cross-field failure is reported;
8. `--show-config` redacts secrets;
9. `--check-config` does not bind sockets.

### 17.4 Error/Outcome Tests

- every outcome constructor serializes expected JSON;
- status/code/text mappings are stable;
- internal errors map to expected outcomes;
- redaction removes secrets/body values;
- stats increment for success and rejection classes;
- request logs include bounded canonical fields.

### 17.5 HEC Protocol Tests

- endpoint aliases;
- missing/malformed/invalid auth;
- empty body;
- JSON object/array/number/string/boolean event values;
- absent/null/blank `event`;
- malformed JSON and late invalid concatenated JSON;
- raw LF/CRLF/NUL/control/invalid UTF-8 cases;
- gzip valid/invalid/oversize cases;
- health route;
- ACK disabled behavior.

### 17.6 External Validation

Record:

- command;
- config;
- git revision;
- input path/hash;
- tool and version;
- expected response;
- actual response;
- capture path;
- accepted/rejected/written counts;
- notes comparing Splunk/Vector.

---

## 18. Benchmarking And Performance Recording

Benchmarking must name the stage being measured.

Timing points:

```text
accept -> headers/auth -> body read -> gzip decode -> parse/framing -> enqueue -> sink write -> flush/durable state
```

Run metadata:

- git revision;
- binary version;
- build profile;
- OS/kernel;
- CPU model, core count, cache when available;
- memory size;
- effective config with redacted secrets;
- selected listener/splitter/gzip/parser/queue/sink;
- tool and version;
- corpus path/hash;
- start/end timestamps;
- bytes/sec;
- events/sec;
- latency percentiles;
- error counts;
- host observation snapshots when available.

Ledger format:

- append-only JSONL preferred;
- CSV acceptable for quick throughput tables;
- human summaries are derived output, not the source of truth.

Optimization admission rule:

```text
keep scalar correctness path -> add optimized variant -> agreement tests -> benchmark stage -> default only after correctness and measured value
```

Applies to raw splitting, JSON parser alternatives, tokenization, sink batching, and storage layout.

---

## 19. Build, Packaging, And Service Operation

Near-term:

- `cargo fmt` and `cargo clippy` once conventions settle;
- `cargo test` for unit/integration;
- release build for benchmarks;
- local scripts for process tests and host observation.

Production candidate:

- explicit `Cargo.toml` metadata;
- static-ish release binary where practical;
- systemd unit or container entrypoint;
- stderr JSON logs;
- `RUST_BACKTRACE=1` in service environment;
- graceful SIGTERM;
- optional `sd_notify` later;
- health/readiness endpoints;
- documented config file path and examples;
- shell completions generated by `clap_complete` later.

Systemd hardening later:

```text
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=<capture/store paths>
Restart=on-failure
```

Do not over-harden before paths, sinks, and service mode are clear.

---

## 20. Implementation Sequence

The implementation can proceed in phases without defining every production detail first.

### Phase 1 — Mainstay Infrastructure

- Add `clap`, `figment`, `thiserror` or typed local errors.
- Replace config loader with provider chain.
- Implement `--config`, `--show-config`, `--check-config`.
- Add validation and redacted config output.
- Add config precedence tests.

### Phase 2 — Central Error/Message/Outcome Spine

- Add `messages.rs` and tighten `outcome.rs` constructors.
- Add `error.rs` classes for config/startup/request/sink.
- Route handler early returns through central outcome mapping.
- Add outcome serialization and mapping tests.
- Add redaction tests.

### Phase 3 — Raw Framing And Hostile Input

- Add `line_splitter.rs` with scalar behavior.
- Add tests for LF, CRLF, NUL, controls, non-ASCII, invalid UTF-8, long lines, no-final-newline.
- Preserve original and canonical evidence where relevant.

### Phase 4 — Queue/Sink Separation

- Define `EventBatch`, `SinkCommit`, queue policy, and worker loop.
- Add queue-full and sink-failure outcomes.
- Add counters and process tests.

### Phase 5 — Observability And Benchmark Ledger

- Add structured startup/request/shutdown logs.
- Stabilize stats names.
- Add benchmark ledger schema.
- Run single-stream and concurrent load tests.

### Phase 6 — External Compatibility

- Compare selected cases with local Splunk Enterprise.
- Validate Vector as HEC client.
- Record compatibility differences in ledgers.

---

## 21. Acceptance Criteria For Infrastructure Slice

The first infrastructure slice is accepted when:

- config is loaded through `clap` + `figment`, not hand-coded merge logic;
- defaults, file, CLI, and env precedence are tested;
- config validation fails before bind;
- `--show-config` redacts secrets;
- `--check-config` uses the same validation path;
- handlers no longer scatter HEC response text/code;
- request rejection classes have central outcomes, counters, and log fields;
- raw splitter behavior is explicit and tested;
- benchmark and validation runs can be recorded with reproducible metadata.

---

## 22. Open Decisions

These are allowed to remain open while early phases proceed.

| Area | Decision Needed | Blocking? |
| --- | --- | --- |
| Exact invalid-token HTTP status | compare Splunk/Vector | no for config phase |
| Body-too-large HEC code/text | compare Splunk/Vector | no for config phase |
| ACK disabled response | compare Splunk/Vector | no until ACK route is used |
| JSON partial success vs all-or-nothing | compare Splunk and durability preference | no for config phase; yes before event parser finalization |
| Prometheus adoption | external metrics requirement | no |
| Dedicated observability port | production operation mode | no |
| Direct Hyper accept loop | Axum limitation evidence | no |
| Durable sink format | ACK/replay requirement | no |

---

## 23. References

Current controlling docs:

- `/Users/walter/Work/Spank/HECpoc/HECpoc.md` — product scope and implementation sequence;
- `/Users/walter/Work/Spank/HECpoc/Config.md` — detailed config contract;
- `/Users/walter/Work/Spank/HECpoc/ErrorMessaging.md` — detailed error/outcome/message spec;
- `/Users/walter/Work/Spank/HECpoc/Stack.md` — detailed HTTP/Tokio/Axum and backpressure findings;
- `/Users/walter/Work/Spank/HECpoc/PerfIntake.md` — performance distillation;
- `/Users/walter/Work/Spank/HECpoc/History.md` — non-authoritative historical pointers.

Reviewed infrastructure source patterns:

- `/Users/walter/Work/Spank/infra/Infrastructure.md` — operations layers and scale framing;
- `/Users/walter/Work/Spank/spank-py/Infra.md` — cross-cutting infrastructure requirements and call-site conventions;
- `/Users/walter/Work/Spank/spank-rs/research/Infrust.md` — Rust infrastructure mandates.
