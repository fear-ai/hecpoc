# InfraHEC — HECpoc Infrastructure Spine

`InfraHEC.md` concentrates the cross-cutting infrastructure plan for the HECpoc Rust implementation: runtime, configuration, errors, outcomes, reporting, public text, metrics, lifecycle, validation, benchmarking, security posture, and operational packaging. It adopts the best layout ideas from:

- `/Users/walter/Work/Spank/infra/Infrastructure.md` — functional layers, scale regimes, protocol selection, and operations framing;
- `/Users/walter/Work/Spank/spank-py/Infra.md` — problem/benefit/requirements/architecture/decision sections, call-site conventions, metrics/health, security posture, and validation survey style;
- `/Users/walter/Work/Spank/spank-rs/research/Infrust.md` — Rust-specific mandates for `tracing`, metrics, error taxonomy, `figment`, lifecycle, Tokio runtime, CLI, health, testing, and build tooling;
- current HECpoc documents: `HECpoc.md`, `InfraHEC.md`, `Stack.md`, and `docs/PerfIntake.md`.

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
| `Stack.md` | HTTP/Tokio/Axum, Tower avoidance, backpressure, byte stages | Fold into runtime, HTTP stack, body processing, and resilience sections |
| `docs/PerfIntake.md` | Performance caution and benchmark orientation | Use benchmark evidence to admit optimizations |

Preferred section cadence:

1. problem and benefit;
2. requirements;
3. architecture or implementation shape;
4. call-site conventions where relevant;
5. validation and acceptance;
6. open decisions only when the decision is genuinely not yet made.

History and abandoned approaches do not belong in the main flow. They can be noted in `docs/History.md` when needed.

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
  Stack.md
  docs/
    History.md
    PerfIntake.md
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
      outcome.rs
      parse_event.rs
      parse_raw.rs
      report.rs
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
  report.rs       report definitions, report records, redaction, routing
  public_text.rs  optional home for public text if outcome/report types need it
  stats.rs        counters and snapshots
```

Semantic boundary:

- `error.rs` classifies failures;
- `outcome.rs` defines client responses;
- `report.rs` defines occurrence/report vocabulary and output routing;
- `public_text.rs` is added only if public text needs a separate module;
- HTTP handlers convert errors to outcomes at the adapter edge;
- stats/logging happen through report definitions and outcome mappings, not direct handler updates.

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

| Internal condition | HEC outcome | Counter reason | Default severity |
| --- | --- | --- | --- |
| missing auth token | `token_required()` | `token_required` | `warn` |
| malformed auth header | `invalid_authorization()` | `invalid_authorization` | `warn` |
| invalid token | `invalid_token()` | `invalid_token` | `warn` |
| empty body | `no_data()` | `no_data` | `info` |
| invalid JSON | `invalid_data_format()` | `invalid_json` | `info`/`warn` |
| gzip decode failed | `invalid_data_format()` | `gzip_decode_failed` | `warn` |
| queue full | `server_busy()` | `queue_full` | `warn` |
| sink unavailable before acceptance | `server_busy()` | `sink_unavailable` | `error` |

Every mapped outcome has a central reason, HEC response mapping, counter effect, and report definition. The route code should not independently update stats and logs for the same occurrence.

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
report.emit(HEC_AUTH_HEADER_MALFORMED.at(&ctx).field("problem", AuthHeaderProblem::NonText));
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

## 9. Reporting, Logging, And Observability

### 9.1 Communication Vocabulary

Avoid using `message` as the root concept. In this project it can mean a HEC protocol response, an internal error string, a structured log record, a metric update, a CLI response, or a future notification. Those are related, but not the same object.

Use this vocabulary:

| Term | Meaning | Example |
| --- | --- | --- |
| occurrence | Something meaningful happened inside the system | request arrived, token invalid, body decoded, sink commit failed |
| report definition | Code-owned schema and policy for one occurrence type | `HEC_AUTH_TOKEN_INVALID` |
| report record | One runtime instance of a report definition plus fields | token invalid for request id `42` |
| outcome | Protocol-facing or operation-facing result | HEC code `4`, HTTP `401`, queue full, sink commit failed |
| error | Internal typed failure object | `AuthError::MalformedHeader`, `SinkError::WriteFailed` |
| output sink | Where rendered information goes | stderr, stdout, file, stats counters, benchmark ledger |
| renderer/backend | How a report record becomes bytes or counter updates | compact log, JSON log, `tracing`, stats update |
| public text | Human/client-visible text chosen by policy | `Invalid token`, `hec configuration ok` |

Call-site communication should usually start from an occurrence or typed error, not from output text. Output text is a rendering result.

### 9.2 Design Principle

Call sites report a named functional occurrence with structured context. They should not decide whether that occurrence becomes a log line, counter, terminal/stderr message, benchmark record, trace span, or future external notification.

The goal is not to replace forty scattered call-site shapes with five arbitrary call-site shapes. The goal is one regular reporting model with typed definitions, runtime routing, and enough structured fields to support logs, status, counters, performance records, and later analysis.

Avoid all three failure modes:

- do not scatter backend-specific `tracing::info!`, `tracing::warn!`, metrics updates, stderr writes, and ledger writes through ordinary handlers;
- do not hide all behavior behind a vague `msg(subsystem, level, ...)` function that loses domain meaning;
- do not invent separate verb families for every surface result, such as `reject`, `fail`, `measure`, `terminal`, and then let those families drift.

Preferred call-site shape:

```rust
report.emit(HEC_AUTH_TOKEN_INVALID
    .at(&ctx)
    .field("auth_scheme", parsed.scheme())
    .field("token_present", parsed.token_present())
    .duration("auth_us", started.elapsed()));

report.emit(HEC_BODY_DECODED
    .at(&ctx)
    .bytes("wire_bytes", wire_len)
    .bytes("decoded_bytes", decoded_len)
    .duration("decode_us", started.elapsed()));

report.emit(HEC_SINK_COMMIT_FAILED
    .at(&ctx)
    .state("sink_state", SinkState::CommitAttempted)
    .error_class(error.class())
    .field("sink_kind", sink.kind()));
```

Here `HEC_AUTH_TOKEN_INVALID`, `HEC_BODY_DECODED`, and `HEC_SINK_COMMIT_FAILED` are static report definitions. A definition owns the stable name, phase, component, step, default severity, outcome class, default output routing, counter updates, and allowed/redacted fields. The call site supplies runtime values.

For unexpected or investigation-specific diagnostics, keep the same shape but allow a dynamic diagnostic definition:

```rust
if report.enabled(HEC_BODY_SPLITTER_DETAIL) {
    report.emit(HEC_BODY_SPLITTER_DETAIL
        .at(&ctx)
        .field("line_breaker", splitter.kind())
        .field("byte_class", byte_class)
        .offset("byte_offset", offset));
}
```

Diagnostic coverage cannot be complete at design time. The design requirement is that diagnostics still use the same source, phase, component, severity, redaction, and output-routing machinery rather than becoming ad hoc `println!` or backend calls.

The reporting component fans out to configured outputs without changing the call site:

```text
report definition + runtime fields
  -> logs/tracing
  -> counters
  -> stderr/stdout/file output where configured
  -> benchmark/performance ledger where configured
  -> future external notification where configured
```

Terminal output is not a special reporting concept merely because it is a terminal. It is another file descriptor or output sink. It becomes a separate concern only for interactive command responses, paging, prompts, TTY detection, color, or human-oriented command rendering.

### 9.3 Backend Stack

Use `tracing` and `tracing-subscriber` as the first logging/tracing backend, not as the application-level observability API.

Initial backend support:

- compact or JSON stderr output;
- runtime-configured default severity;
- runtime-configured source/component filters;
- redaction policy applied before sensitive values reach backend fields;
- no per-event backend log calls on hot ingest paths unless explicitly enabled.

Compile-time log filtering is not the design center. The interesting tuning must happen at runtime and per deployment/debugging situation. Release-build compile-time filters may be considered later only as an optimization after the reporting API is stable.

### 9.4 Report Definitions

A report definition is the stable, code-owned description of one meaningful occurrence.

Minimum definition fields:

| Field | Purpose |
| --- | --- |
| `name` | Stable dotted name, such as `hec.auth.token_invalid`. |
| `phase` | Broad lifecycle area: startup, ingress, decode, parse, queue, sink, shutdown. |
| `component` | Owning implementation component, such as auth, body, gzip, queue, sink. |
| `step` | Functional step inside the phase/component. |
| `default_severity` | Runtime-overridable severity. |
| `kind` | Arrival, condition, state transition, outcome, diagnostic, performance sample. |
| `outcome_class` | Accepted, rejected, failed, skipped, throttled, recovered, informational, or not applicable. |
| `counter_effect` | Optional counter increments and labels. |
| `output_policy` | Default routing to log, status output, benchmark ledger, metrics, or none. |
| `redaction_policy` | Allowed fields, redacted fields, and raw-value prohibition. |

Example definitions:

```rust
pub const HEC_REQUEST_ARRIVED: ReportDef = ReportDef::new("hec.request.arrived")
    .phase(Phase::Ingress)
    .component(Component::Receiver)
    .step(Step::RequestArrived)
    .kind(ReportKind::Arrival)
    .default_severity(Severity::Debug);

pub const HEC_AUTH_TOKEN_INVALID: ReportDef = ReportDef::new("hec.auth.token_invalid")
    .phase(Phase::Ingress)
    .component(Component::Auth)
    .step(Step::Authorize)
    .kind(ReportKind::Outcome)
    .outcome_class(OutcomeClass::Rejected)
    .default_severity(Severity::Warn)
    .counter("requests_rejected_total", "reason", "invalid_token");

pub const HEC_SINK_COMMIT_FAILED: ReportDef = ReportDef::new("hec.sink.commit_failed")
    .phase(Phase::Sink)
    .component(Component::Sink)
    .step(Step::Commit)
    .kind(ReportKind::Outcome)
    .outcome_class(OutcomeClass::Failed)
    .default_severity(Severity::Error)
    .counter("sink_failures_total", "reason", "commit_failed");
```

The exact Rust construction syntax may change if `const` initialization or field storage pushes toward plain struct literals, macros, or generated tables. The stable requirement is the call-site contract: named definition plus typed runtime fields, with routing/redaction/counter policy centralized.

`Rejected` and `Failed` are not separate messaging functions. They are outcome classes on one report record model:

- rejected means the receiver intentionally refuses or stops processing due to input, auth, policy, limit, or compatibility handling; this is often client-visible and expected under hostile input;
- failed means the receiver tried to perform an intended operation and could not complete it due to internal error, dependency failure, resource exhaustion, or a violated invariant;
- both can become logs, counters, HEC responses, status records, and test assertions through the same reporting and outcome mapping.

### 9.5 Sources, Severity, And Filtering

Application source is not the same thing as a `tracing` target. Source is the product/component origin of an occurrence; target is the backend routing string.

Initial components/sources:

```text
hec
hec.config
hec.runtime
hec.auth
hec.body
hec.gzip
hec.event_parser
hec.raw_parser
hec.queue
hec.sink
hec.stats
```

Initial severities:

```text
trace, debug, info, warn, error
```

Config shape:

```toml
[observe]
level = "info"
format = "compact"
redaction_mode = "redact"
redaction_text = "<redacted>"

[observe.sources]
hec.auth = "debug"
hec.body = "info"
hec.queue = "warn"
hec.sink = "debug"

[observe.outputs]
logs = true
stats = true
status = true
benchmark_ledger = false
```

The reporter maps source and severity to backend `tracing::event!` calls internally. Ordinary call sites should not hand-type backend targets or log-level macros for product-significant events.

### 9.6 Startup Occurrences

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

### 9.7 Request And Processing Occurrences

Request outcome records need enough structure for user logs, tests, counters, and post-processing:

- `name`, such as `hec.request.completed` or `hec.auth.token_invalid`;
- `phase`, `component`, and `step`;
- `kind`, such as arrival, condition, state transition, outcome, diagnostic, or performance sample;
- `outcome_class`, when applicable;
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

Performance and duration records must be structured rather than text-only:

- durations use explicit units in field names or typed fields, such as `decode_us`, `auth_us`, `sink_commit_us`;
- byte and event counts use typed integer fields;
- state records identify `state_from`, `state_to`, and reason where a true transition exists;
- hot-path detailed diagnostics require `report.enabled(definition)` guards before expensive field construction.

### 9.8 Shutdown Occurrences

Fields:

- `event="shutdown"`;
- `reason`;
- `uptime_ms`;
- final counters;
- worker join status;
- flush status.

### 9.9 Command Output And Human Display

Stdout, stderr, terminals, files, and future local sockets are output sinks. A terminal is not special for routine reporting.

Command output is separate only when the program is answering a direct CLI command such as `--show-config` or `--check-config`. That path should share redaction, formatting, and message text definitions with reporting, but it should not create one-off terminal methods per command:

```rust
output.write(CommandResponse::EffectiveConfig(config.redacted()));
output.write(CommandResponse::ConfigCheckOk);
output.write(CommandResponse::StartupFailure(error.to_public()));
```

If the project later adds interactive operation, that is a larger UI/session concern: TTY detection, color, paging, prompts, refresh, confirmation, interruption, and possibly separate human-readable views. The output file descriptor is the trivial part.

### 9.10 Call-Site Contract

Call sites should be easy to audit:

- report product-significant occurrences with `report.emit(def.at(&ctx)...);`
- build expensive diagnostic fields only after `report.enabled(def)`;
- return typed errors or outcomes separately from report rendering;
- avoid raw public text at protocol and infrastructure call sites;
- avoid direct output writes except at the final command-output adapter;
- avoid direct counter updates when the occurrence already has a report definition with counter effects.

Good:

```rust
let outcome = outcomes::invalid_token();
report.emit(HEC_AUTH_TOKEN_INVALID
    .at(&ctx)
    .field("auth_scheme", parsed.scheme())
    .outcome(&outcome));
return outcome.into_response();
```

Acceptable for diagnostics:

```rust
if report.enabled(HEC_BODY_SPLITTER_DETAIL) {
    report.emit(HEC_BODY_SPLITTER_DETAIL
        .at(&ctx)
        .offset("byte_offset", offset)
        .field("byte_class", byte_class));
}
```

Avoid:

```rust
tracing::warn!("invalid token");
stats.auth_errors_total.fetch_add(1, Ordering::Relaxed);
eprintln!("Invalid token");
```

### 9.11 Implementation Direction

Likely implementation steps:

1. Define `Phase`, `Component`, `Step`, `ReportKind`, `OutcomeClass`, `Severity`, and `ReportDef`.
2. Define `ReportRecord` as a borrowed static definition plus typed runtime fields.
3. Define `Reporter::emit(record)` and `Reporter::enabled(definition)`.
4. Map report definitions to `tracing` internally; do not expose `tracing::info!` at product call sites.
5. Add output sinks for compact log, JSON log, stats counters, and benchmark/performance ledger.
6. Add command-output rendering for `--show-config`, `--check-config`, and startup failure without one method per CLI command.
7. Add tests for definition names, redaction, outcome class mapping, counter effects, output routing, and disabled hot-path diagnostics.

Critique to preserve:

- a single `emit` shape can become too generic if definitions are weak; definitions must carry phase/component/step/kind/outcome metadata;
- a large enum of every possible event can become rigid; keep stable definitions for product-significant occurrences and allow controlled diagnostics;
- benchmark/performance records are related but may need a separate output sink and schema for post-processing;
- user-visible HEC responses remain protocol outcomes, not log messages, even when they are derived from the same cause/reason definitions.

### 9.10 Health And Readiness

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

## 13. HTTP Stack Interface

Infrastructure decision: start with Tokio plus Axum, with Axum kept at the HTTP adapter boundary. Protocol-critical behavior remains in HECpoc modules, not Tower middleware.

Boundary contract:

| Concern | InfraHEC owns | Stack owns |
| --- | --- | --- |
| Runtime choice | Tokio + Axum as starting infrastructure | exact Tokio/Axum/Hyper accept and body mechanics in `Stack.md §§6–12, 28` |
| HTTP adapter | route to handler, expose request facts, convert final outcome | framework limitations, extractor hazards, Hyper fallback evidence in `Stack.md §§2–8, 20` |
| Protocol-critical controls | auth/body/decode/parse/outcome must be explicit HEC code paths | detailed auth/gzip/body-limit/timeout mechanics in `Stack.md §§9–12` |
| Request phase names | stable phase vocabulary for logs, stats, validation | byte and buffer behavior by phase in `Stack.md §§32, 35–36` |

Request phase contract:

```text
route alias -> endpoint kind -> auth -> bounded body -> optional decode -> endpoint parse/framing -> EventBatch -> sink/queue -> HecOutcome
```

`InfraHEC.md` should not duplicate crate-source findings, kernel details, copy/buffer analysis, or accept-loop research. Those remain in `Stack.md`.

## 14. Sink, Queue, And Inspection Interface

Infrastructure decision: the first correctness sink is local capture; queue and durable states are introduced only when the current phase needs them.

Commit-state vocabulary shared by implementation, logs, stats, and validation:

| State | Meaning |
| --- | --- |
| parsed | request syntax accepted |
| accepted | valid events formed |
| queued | batch entered bounded handoff |
| captured | sink write returned |
| flushed | userspace writer flushed |
| durable | fsync/DB durable commit complete |

Interface rules:

- HTTP success must not imply a stronger commit state than the selected sink mode reaches.
- Direct capture is acceptable for fixture mode.
- Bounded queue is required before advertising backpressure behavior beyond request/body rejection.
- Inspection starts from capture evidence, not from a search/index abstraction.
- Durable storage and ACK wait until `durable` has a real implementation and tests.

Detailed file format, buffering, sink backpressure, and future store mechanics belong in `Stack.md §§33, 35` or a future focused store document.

## 15. Backpressure And Resilience Interface

Infrastructure decision: resource limits and overload behavior must be named, configurable where appropriate, visible in outcomes, and counted.

Required contract:

- each bounded layer has a limit name, failure reason, outcome mapping, counter, and log severity;
- initial bounded layers are wire bytes, decoded bytes, event count, body read time, and sink handoff once queue exists;
- future bounded layers include listener backlog, connection count, per-IP contribution, line bytes, field count, and durable sink pressure;
- default behavior is explicit rejection or server-busy response, not indefinite wait or silent drop;
- hostile input must not crash the process.

Deep mechanics remain in `Stack.md`: network-to-store buffer chain in `Stack.md §35`, kernel/runtime knobs in `Stack.md §30`, ingress resilience policy in `Stack.md §31`, and raw splitting semantics in `Stack.md §36`.

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

### Phase 1 — Mainstay Configuration Infrastructure

- Done: add `clap`, `figment`, and `thiserror`.
- Done: replace config loader with provider chain.
- Done: implement `--config`, `--show-config`, `--check-config`.
- Done: add validation and redacted config output.
- Done: add config precedence tests.

Implemented configuration state:

- provider precedence is compiled defaults < TOML config file < CLI flags < environment variables;
- `--config` selects the config file, with environment support for configured deployment paths;
- `--show-config` prints redacted effective TOML;
- `--check-config` runs the same validation path without starting the receiver;
- validation covers token, bind address, byte/event limits, duration bounds, gzip buffer range, and environment parse failures.

Relevant files:

- `/Users/walter/Work/Spank/HECpoc/Cargo.toml` — `clap`, `figment`, and `thiserror`;
- `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/config.rs` — layered config implementation and tests;
- `/Users/walter/Work/Spank/HECpoc/src/main.rs` — config action handling;
- `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/protocol.rs` — old environment parsing removed;
- `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/mod.rs` — `ConfigAction` export.

Validation evidence saved under `/Users/walter/Work/Spank/HECpoc/results/`:

- `test-list-20260504T021751Z.txt` — 34 tests listed;
- `test-output-20260504T021751Z.txt` — 34 tests passed;
- `check-config-20260504T021804Z.log` — `--check-config` output;
- `show-config-20260504T021804Z.log` — redacted `--show-config` output;
- `startup-20260504T021804Z.log` — startup status output;
- `bench-ab-single-20260504T021828Z.txt` — local `ab -n 1000 -c 1` smoke output;
- `bench-ab-c50-20260504T021828Z.txt` — local `ab -n 5000 -c 50` smoke output;
- `bench-stats-20260504T021828Z.json` — stats after smoke runs.

Smoke results are not capacity claims. They only prove the receiver starts, accepts raw HEC traffic locally, and counters remain clean under small release-mode `ab` runs.

### Phase 2 — Reporting, Error, And Outcome Spine

- Add `report.rs` with `Reporter`, `ReportDef`, `ReportRecord`, `Phase`, `Component`, `Step`, `ReportKind`, `OutcomeClass`, `Severity`, and redaction policy.
- Add static report definitions for startup, config, request arrival, auth outcomes, body/gzip outcomes, parser outcomes, queue outcomes, sink outcomes, shutdown, and selected performance records.
- Add controlled dynamic diagnostic support for investigation-specific records that still use source, phase, component, severity, redaction, and output routing.
- Avoid a generic `messages.rs` dumping ground. Add `public_text.rs`, `render.rs`, or `output.rs` only if public text and command-output rendering need a separate home after report/outcome types exist.
- Tighten `outcome.rs` constructors.
- Add `error.rs` classes for config/startup/request/sink.
- Route handler early returns through central outcome mapping and `Reporter::emit(...)` records.
- Add outcome serialization and mapping tests.
- Add report definition, redaction, counter-effect, routing, and disabled-diagnostic tests.
- Add runtime-configured reporting source filters and compact/json backend output.

### Phase 3 — Raw Framing And Hostile Input

- Add `line_splitter.rs` with scalar behavior.
- Add tests for LF, CRLF, NUL, controls, non-ASCII, invalid UTF-8, long lines, no-final-newline.
- Preserve original and canonical evidence where relevant.

### Phase 4 — Queue/Sink Separation

- Define `EventBatch`, `SinkCommit`, queue policy, and worker loop.
- Add queue-full and sink-failure outcomes.
- Add counters and process tests.

### Phase 5 — Observability And Benchmark Ledger

- Expand reporter outputs for structured startup/request/shutdown events.
- Stabilize stats names.
- Add benchmark ledger schema.
- Run single-stream and concurrent load tests.

### Phase 6 — External Compatibility

- Compare selected cases with local Splunk Enterprise.
- Validate Vector as HEC client.
- Record compatibility differences in ledgers.

---

## 21. Acceptance Criteria For Infrastructure Phase

The first infrastructure phase is accepted when:

- config is loaded through `clap` + `figment`, not hand-coded merge logic;
- defaults, file, CLI, and env precedence are tested;
- config validation fails before bind;
- `--show-config` redacts secrets;
- `--check-config` uses the same validation path;
- handlers no longer scatter HEC response text/code;
- product-significant call sites use static report definitions and `Reporter::emit(...)` rather than direct backend logging calls;
- rejected and failed outcomes are represented as outcome classes, not separate messaging APIs;
- request rejection classes have central outcomes, counters, and reporting fields;
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
- `/Users/walter/Work/Spank/HECpoc/Stack.md` — detailed HTTP/Tokio/Axum and backpressure findings;
- `/Users/walter/Work/Spank/HECpoc/docs/PerfIntake.md` — performance distillation;
- `/Users/walter/Work/Spank/HECpoc/docs/History.md` — non-authoritative historical pointers.

Reviewed infrastructure source patterns:

- `/Users/walter/Work/Spank/infra/Infrastructure.md` — operations layers and scale framing;
- `/Users/walter/Work/Spank/spank-py/Infra.md` — cross-cutting infrastructure requirements and call-site conventions;
- `/Users/walter/Work/Spank/spank-rs/research/Infrust.md` — Rust infrastructure mandates.
