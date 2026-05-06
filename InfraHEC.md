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
- Tokio provides socket/runtime primitives; Axum currently owns the server accept loop; Hyper owns HTTP parsing. Detailed accept/read mechanics remain in `Stack.md`, not in generic infrastructure.

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

Observation and reporting settings are first-class configuration, not hard-coded debug switches:

| Parameter | TOML key | Env var | CLI flag | Default | Validation |
| --- | --- | --- | --- | --- | --- |
| Observe level/filter | `observe.level` | `HEC_OBSERVE_LEVEL` | `--observe-level` | `info` | valid `tracing-subscriber` filter syntax |
| Observe format | `observe.format` | `HEC_OBSERVE_FORMAT` | `--observe-format` | `compact` | `compact` or `json` |
| Redaction mode | `observe.redaction_mode` | `HEC_OBSERVE_REDACTION_MODE` | `--observe-redaction-mode` | `redact` | `redact` or `passthrough` |
| Redaction text | `observe.redaction_text` | `HEC_OBSERVE_REDACTION_TEXT` | `--observe-redaction-text` | `<redacted>` | non-empty |
| Tracing output | `observe.tracing` | `HEC_OBSERVE_TRACING` | `--observe-tracing <bool>` | `true` | boolean |
| Console output | `observe.console` | `HEC_OBSERVE_CONSOLE` | `--observe-console <bool>` | `false` | boolean |
| Stats output | `observe.stats` | `HEC_OBSERVE_STATS` | `--observe-stats <bool>` | `true` | boolean |

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
- secrets are redacted by default; explicit `observe.redaction_mode = "passthrough"` is an operator/debugging override and must be visible in the effective config output.

Configuration prompt guardrail:

```text
When adding any new runtime behavior, add its typed config field, TOML key,
CLI flag, env var, compiled default, validation rule, redacted show-config
behavior, and precedence test in the same change. Do not add hidden constants
or one-off environment reads in domain modules.
```

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
  report.rs       Reporter pipeline, redaction, routing, rendering backends
  public_text.rs  optional home for public text if outcome/report types need it
  stats.rs        counters and snapshots
```

Semantic boundary:

- `error.rs` classifies failures;
- `outcome.rs` defines client responses;
- `report.rs` implements the Reporter pipeline and consumes domain-owned submitted facts;
- `public_text.rs` is added only if public text needs a separate module;
- HTTP handlers convert errors to outcomes at the adapter edge;
- stats/logging happen through domain submitted facts and outcome mappings, not direct handler updates.

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

Central client-visible type:

```rust
pub struct HecResponse {
    pub status: StatusCode,
    pub text: &'static str,
    pub code: u16,
    pub ack_id: Option<u64>,
    pub invalid_event_number: Option<usize>,
}
```

Naming decision: use `Outcome` for accepted/rejected/failed/skipped/throttled/recovered operation disposition and `HecResponse` for the client-visible HEC response body/status/code. Do not introduce `HecOutcome` in new code; it conflates protocol response with operation disposition.

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
AuthError -> HecResponse
BodyError -> HecResponse
DecodeError -> HecResponse
ParseError -> HecResponse
QueueError -> HecResponse
SinkError + SinkCommitState -> HecResponse
```

Target spelling is `HecResponse` in code and documentation. Any remaining `HecOutcome` reference is historical wording to remove, not a type to copy.

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

Every mapped outcome has a central reason, HEC response mapping, counter effect, and submitted fact name/fields. The route code should not independently update stats and logs for the same fact.

Error-handling prompt guardrail:

```text
For every new failure path, name the internal error, public HEC response,
operation Outcome, report fact, allowed fields, counter effect, severity,
and validation test. Do not let the Reporter infer HEC behavior, peer data,
protocol codes, or counter reasons from generic context.
```

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
let outcome = Outcome::Rejected;
let response = HecResponse::invalid_authorization().with_reason(AuthReason::MalformedHeader);
report.submit(&ctx, auth::HEADER_MALFORMED, fields![field::auth_problem(AuthHeaderProblem::NonText)]);
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

### 9.1 Communication Layers

Avoid using `message` as the root concept. In this project it can mean a HEC protocol response, an internal error string, a structured log record, a metric update, console text, a benchmark row, or a future notification. Those are related, but they belong to different layers.

Use layers, not another family of `Report*` or `Occurrence*` abstractions:

| Layer | Owned by | Examples | Notes |
| --- | --- | --- | --- |
| Domain state/result | Auth, body, parser, queue, sink, lifecycle modules | token invalid, body decoded, queue full, sink commit failed | Specific semantics live here, not in Reporter. |
| Outcome | Protocol/operation mapping | accepted, rejected, failed, skipped, throttled, recovered | Use `Outcome`; if HEC client response needs a type, prefer `HecResponse` over overloading `Outcome`. |
| Submission | Call site plus Reporter input adapter | domain fact name, context, fields, duration, outcome | A structured fact handed to Reporter. It should be light and auditable. |
| Reporter pipeline | Reporting subsystem | filter, redact, route, transform, fan out | End-to-end subsystem from submission to persistence/display. |
| Output products | Output adapters | log entry, console line, metric update, benchmark row, status record | Multiple products may be derived from one submitted fact. |
| Backends | Libraries/devices/files | `tracing`, stdout/stderr, JSONL file, stats counters | `tracing` is useful but not exclusive. |

`HEC_AUTH_TOKEN_INVALID` is therefore not a Reporter concept. It is an auth/protocol fact name or domain status. Its allowed fields, HEC response mapping, metrics labels, and reporting use should be bounded by auth/protocol design, while Reporter treats it as a structured submission to filter, redact, route, and render.

### 9.2 Reporter Subsystem

Reporter is the end-to-end subsystem from call-site initiation to persistence and/or display. It owns:

- runtime filtering by component/source, severity, outcome, and output;
- redaction and field allow/deny policy enforcement;
- transformation from submitted facts into output-specific products;
- routing to logs, console, counters, status output, benchmark ledgers, and future notification sinks;
- backend adapters, including but not limited to `tracing`.

Reporter must not own HEC-specific semantics such as "invalid token means HEC code 4" or "queue full maps to server busy". Those belong in protocol/outcome modules. Reporter may receive those values as fields.

Preferred call-site style:

```rust
let outcome = Outcome::Rejected;
let response = HecResponse::invalid_token();

report.submit(
    &ctx,
    auth::TOKEN_INVALID,
    fields![
        field::outcome(outcome),
        field::auth_scheme(parsed.scheme()),
        field::token_present(parsed.token_present()),
        field::hec_code(response.code()),
        field::auth_len(parsed.token_len()),
        field::elapsed_us(started.elapsed()),
    ],
);

return response.into_response();
```

For diagnostics:

```rust
report.submit_lazy(&ctx, body::SPLITTER_DETAIL, || {
    fields![
        field::line_breaker(splitter.kind()),
        field::input_class(input_class),
        field::input_offset(offset),
    ]
});
```

The exact Rust representation may be plain structs, constants, macros, or generated tables. The stable point is the layer boundary: domain modules name and bound their statuses/results; Reporter receives structured submissions and handles output.

`submit_lazy` is for expensive or highly verbose diagnostic paths. The fact name appears once, and field construction is skipped when the fact is disabled.

Fact constants should not construct a heap object at runtime. Use either a small copyable id into a static registry or a reference to static metadata:

```rust
pub const TOKEN_INVALID: FactId = FactId(17);

pub static FACTS: &[FactSpec] = &[
    FactSpec {
        id: TOKEN_INVALID,
        name: "hec.auth.token_invalid",
        phase: Phase::Ingress,
        component: Component::Auth,
        step: Step::Authorize,
        level: Severity::Warn,
        outputs: outputs::LOG | outputs::CONSOLE | outputs::STATS,
        counters: &[counter::REQUESTS_REJECTED_TOTAL.with_reason(reason::INVALID_TOKEN)],
        fields: &[field::OUTCOME, field::AUTH_SCHEME, field::TOKEN_PRESENT, field::HEC_CODE, field::AUTH_LEN, field::ELAPSED_US],
    },
];
```

The call site passes `FactId` plus typed field values. Reporter uses the registry for metadata, routing, allowed fields, default outputs, and counter mapping.

### 9.3 Fan-Out Semantics

One submitted fact can produce several output products:

```text
auth token invalid submission
  -> structured log entry
  -> requests_rejected_total counter update
  -> console warning when enabled
  -> benchmark/security ledger row when enabled
```

That is one submission and multiple rendered/output records. It is not four separate submissions.

Separate submissions are appropriate when separate facts are observed at different points:

```text
request arrived
body decoded
auth token invalid
request completed
```

These may share request id, peer address, route alias, and timing context, but they are distinct submissions because they correspond to different processing steps.

### 9.4 Backends And Outputs

Use `tracing` and `tracing-subscriber` as the first structured logging/tracing backend because they are the Rust ecosystem's standard substrate for levels, structured fields, spans, subscribers, compact output, and JSON output. This is an implementation backend choice, not the reporting model.

Initial output support:

- `tracing` compact or JSON logs;
- console output to stdout or stderr;
- in-process counters/stats;
- status output for direct commands such as `--show-config` and `--check-config`;
- benchmark/performance JSONL ledger when enabled.

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
tracing = true
console = true
stats = true
status = true
benchmark_ledger = false

[observe.console]
stream = "stderr"
format = "compact"
color = "auto"
interactive = false
```

Console output is an output option, not a special call-site API. It becomes a separate interactive subsystem only if prompts, paging, refresh, terminal capabilities, or user sessions are introduced.

Initial library choices and adapter syntax:

| Output | Library / mechanism | Adapter call, not product call site |
| --- | --- | --- |
| tracing log | `tracing`, `tracing-subscriber` | `tracing::event!(target: fact.target(), level, name = fact.name(), request_id = %ctx.request_id(), fields = %record.redacted_json())` |
| console | `std::io::{stdout, stderr}` initially; `anstream` only if color portability is needed | `writeln!(console.stream(), "{}", render_console(record))` |
| stats counters | current `Stats` with `AtomicU64`; `metrics`/Prometheus later | `stats.increment(counter_id, labels)` from fact counter mapping |
| status command output | `std::io` plus existing TOML/JSON renderers | `output.write(CommandResponse::EffectiveConfig(config.redacted()))` |
| benchmark ledger | `serde`, `serde_json`, file writer or background Tokio writer | `serde_json::to_writer(&mut ledger, &BenchmarkRow::from(record))` |

The product call site remains `report.submit(...)`; output adapters are internal Reporter implementation.

Current implementation state:

- `RuntimeConfig.observe` carries level, format, redaction mode/text, and output booleans.
- `main.rs` initializes `tracing-subscriber` after configuration load and before socket bind.
- `AppState` constructs `Reporter` from configured output booleans.
- `Reporter` routes each submitted fact to enabled tracing, console, and stats outputs according to registry defaults and runtime output toggles.
- `ReportContext` currently carries only a request id; route aliases, endpoint kind, HEC code, HTTP status, byte lengths, and elapsed duration are explicit submitted fields.

Example translation for `auth::TOKEN_INVALID`:

```text
call site:
  report.submit(ctx, auth::TOKEN_INVALID, [outcome=Rejected, hec_code=4, auth_scheme=Splunk])

registry lookup:
  name=hec.auth.token_invalid
  component=Auth
  step=Authorize
  severity=Warn
  outputs=LOG|CONSOLE|STATS
  counters=requests_rejected_total{reason=invalid_token}

Reporter fan-out:
  tracing: event target="hec.auth" level=WARN name="hec.auth.token_invalid" hec_code=4 auth_scheme="Splunk"
  console: "WARN hec.auth.token_invalid phase=ingress component=auth step=authorize request=<id> fields={...}"
  stats: increment requests_rejected_total with reason=invalid_token
  ledger: no row unless benchmark/security ledger output enabled
```

### 9.5 Fields, Context, And Domain Fact Names

Common fields should be bounded and typed. Prefer specialized field constructors over generic string-key fields:

- `name`, such as `hec.auth.token_invalid`;
- `phase`, `component`, and `step`;
- `outcome`, such as accepted, rejected, failed, skipped, throttled, recovered, informational;
- `severity`;
- `request_id` when available;
- `peer_addr`;
- `method`;
- `route_alias`;
- `endpoint_kind`;
- `status`;
- `hec_code`;
- `reason`;
- `wire_len`;
- `decoded_len`;
- `event_count`;
- `elapsed_us`;
- `state_from` and `state_to` for true state transitions;
- `sink_commit_state` when reached.

Field primitive type, interpretation, serialization, and formatting are not decided at each call site:

| Layer | Responsibility |
| --- | --- |
| `field::*` constructor | Converts a Rust value into a typed `FieldValue`. |
| field registry | Defines field id, name, primitive type, unit, redaction policy, and display hint. |
| Reporter validation | Verifies the fact permits the field and redacts forbidden values. |
| output adapter | Serializes and formats the field for tracing, console, JSONL, stats, or benchmark output. |

Example field definitions:

```rust
pub const WIRE_LEN: FieldSpec = FieldSpec::u64("wire_len").unit(Unit::Bytes);
pub const DECODED_LEN: FieldSpec = FieldSpec::u64("decoded_len").unit(Unit::Bytes);
pub const EVENT_COUNT: FieldSpec = FieldSpec::u64("event_count").unit(Unit::Count);
pub const ELAPSED_US: FieldSpec = FieldSpec::duration_us("elapsed_us");
pub const INPUT_OFFSET: FieldSpec = FieldSpec::u64("input_offset").unit(Unit::Bytes);
pub const INPUT_CLASS: FieldSpec = FieldSpec::enum_("input_class", &["lf", "crlf", "nul", "control", "non_ascii", "invalid_utf8"]);
```

Example field constructors:

```rust
field::wire_len(wire.len())
field::decoded_len(decoded.len())
field::event_count(events.len())
field::elapsed_us(started.elapsed())
field::input_class(InputClass::Nul)
field::input_offset(offset)
```

`input_class` replaces the vague `byte_class`. It classifies a notable input/framing condition for diagnostics. Initial values should be explicit and finite: `Lf`, `Crlf`, `Nul`, `Control`, `NonAscii`, `InvalidUtf8`, `Oversize`, and `Other`.

`ReportContext` should be explicit, not nebulous. Initial request context fields:

- `request_id`;
- optional domain/adapter fields explicitly submitted by HEC/HTTP code, such as route alias or endpoint kind;
- token id or token hash later, never raw token;
- worker/thread id later only if a subsystem measures and submits it.

Do not let generic Reporter code look up network-specific fields such as peer address, method, endpoint kind, route alias, host, or path. If an output needs those values, the HTTP/HEC adapter submits them as typed fields.

`Instant` decision:

- use `std::time::Instant` for local elapsed-time measurement because it is monotonic and not affected by wall-clock jumps;
- do not store `Instant` in `ReportContext`, persisted records, ledgers, or public output;
- submit elapsed durations as typed fields, such as `field::elapsed_us(started.elapsed())`, only from the subsystem that measured the interval;
- use wall-clock timestamps only for logs/ledgers that need event time, and obtain them at the Reporter/output layer or as an explicit submitted field;
- apply this consistently to request handling, body read/decode, parser timing, sink timing, startup steps, and benchmarks.

Instant pros:

- correct for elapsed latency and timeout measurement because it is monotonic;
- cheap and local;
- immune to NTP, manual clock changes, daylight savings, and wall-clock jumps;
- expresses "how long did this step take" without implying event time.

Instant cons:

- not serializable in a meaningful way;
- not comparable across processes, restarts, hosts, or benchmark runs;
- not suitable for event timestamps, file timestamps, ledger timestamps, or protocol time;
- easy to misuse if hidden inside generic context and later rendered as if it were wall time.

Universal rule: store `Instant` only in the measuring scope, convert to `Duration` at the boundary, and submit/output an explicit unit-bearing field such as `elapsed_us`. Use wall-clock time only for "when did this happen" records.

Phase, component, step, default severity, default outputs, and default field policy should be attached to the domain fact constant, not repeated at every call site:

```rust
pub const TOKEN_INVALID: FactId = FactId(17);
```

Function-specific values such as `splitter.kind()` are acceptable as domain fields. They are supplied by the raw/body subsystem and rendered generically by Reporter.

Performance and duration data must remain structured rather than text-only. Hot-path diagnostics should use `submit_lazy` before expensive field construction.

### 9.6 Call-Site Contract

Call sites should be easy to audit:

- submit product-significant facts through `report.submit(&ctx, domain::FACT, fields![...])`;
- use domain-owned names/constants such as `auth::TOKEN_INVALID`;
- return typed errors or HEC responses separately from report rendering;
- avoid raw public text at protocol and infrastructure call sites;
- avoid direct output writes except at final command-output adapters;
- avoid direct counter updates when the submitted fact already has a counter effect;
- do not call `tracing::info!`, `println!`, `eprintln!`, or benchmark writers directly for product-significant facts.

Prompt guardrail for future design and implementation:

```text
Design generic reporting infrastructure with no built-in knowledge of HTTP, HEC,
auth, peer addresses, endpoints, paths, request timing, protocol codes, or counters.
For every proposed Reporter API, mark each symbol as one of:
generic reporting, HEC domain, HTTP adapter, stats subsystem, output backend.
Reject the design if generic reporting references symbols outside its layer.
Domain modules pass all domain/context fields explicitly.
Reporter filters, redacts, serializes, routes, and dispatches only from submitted
fields and registered output bindings.
```

### 9.7 Implementation Direction

Likely implementation steps:

1. Define `Reporter`, `ReportContext`, `FactId`, `FactSpec`, `FieldSpec`, `FieldValue`, `Phase`, `Component`, `Step`, `Outcome`, and `Severity`.
2. Keep fact constants near owning modules: auth, body, gzip, parser, queue, sink, lifecycle.
3. Rename or separate HEC client response terminology so generic `Outcome` is not confused with protocol response bodies.
4. Add Reporter output adapters for `tracing`, console, stats counters, command/status output, and benchmark ledger.
5. Add runtime-configured source filters, output toggles, and redaction.
6. Add tests for redaction, output fan-out, disabled diagnostics, output routing, typed fields, and outcome-to-HEC-response mapping.

Cross-cutting infrastructure review rule:

```text
Before implementing a new component, write its boundary in the same vocabulary:
config knobs, internal errors, public response if any, operation Outcome,
report facts, typed fields, counter effects, redaction behavior, validation
cases, and benchmark/ledger fields if timing or throughput is claimed.
Then implement the smallest code path that exercises those definitions.
```

### 9.8 Health And Readiness

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
route alias -> endpoint kind -> auth -> bounded body -> optional decode -> endpoint parse/framing -> EventBatch -> sink/queue -> HecResponse
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
- ACK is request/batch scoped and must name its selected commit boundary. `enqueue` is acceptable for explicit benchmark mode; production ACK should wait for `durable` or a similarly tested DB/file commit boundary.
- Durable storage and production ACK wait until `durable` has a real implementation and tests.

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
- error counts split by auth, body-read, body-size, timeout, gzip, parse, sink, and queue classes;
- process CPU, RSS, virtual size, thread count, descriptor count, and elapsed time samples;
- system CPU/load, VM, network socket, and IO snapshots when available.

Current scripts:

- `/Users/walter/Work/Spank/HECpoc/scripts/bench_hec_ab.sh` runs repeatable AB stages with HEC stats snapshots.
- `/Users/walter/Work/Spank/HECpoc/scripts/capture_system_stats.sh` samples process and host statistics during long runs.
- `/Users/walter/Work/Spank/HECpoc/scripts/analyze_bench_run.py` derives request/sec, MiB/sec, event/sec, and failure summaries from a result directory.

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
- Done: add observe/reporting config for tracing, console, stats, format, level/filter, and redaction text/mode.

Implemented configuration state:

- provider precedence is compiled defaults < TOML config file < CLI flags < environment variables;
- `--config` selects the config file, with environment support for configured deployment paths;
- `--show-config` prints redacted effective TOML;
- `--check-config` runs the same validation path without starting the receiver;
- validation covers token, bind address, byte/event limits, duration bounds, gzip buffer range, and environment parse failures.
- observation config covers tracing/console/stats toggles, compact/json tracing format, filter syntax, and configurable redaction text.

Relevant files:

- `/Users/walter/Work/Spank/HECpoc/Cargo.toml` — `clap`, `figment`, and `thiserror`;
- `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/config.rs` — layered config implementation and tests;
- `/Users/walter/Work/Spank/HECpoc/src/main.rs` — config action handling;
- `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/protocol.rs` — old environment parsing removed;
- `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/mod.rs` — `ConfigAction` export.

Validation evidence saved under `/Users/walter/Work/Spank/HECpoc/results/`:

- `test-list-20260504T021751Z.txt` — 34 tests listed;
- `test-output-20260504T021751Z.txt` — 34 tests passed;
- `test-output-20260504T232623Z.txt` — 40 tests passed after observe/reporting config and `HecResponse` alignment;
- `test-output-20260504T232708Z.txt` — 40 tests passed after warning cleanup;
- `check-config-20260504T021804Z.log` — `--check-config` output;
- `check-config-20260504T232708Z.log` — warning-free `--check-config` output after observe/reporting config;
- `show-config-20260504T021804Z.log` — redacted `--show-config` output;
- `startup-20260504T021804Z.log` — startup status output;
- `bench-ab-single-20260504T021828Z.txt` — local `ab -n 1000 -c 1` smoke output;
- `bench-ab-c50-20260504T021828Z.txt` — local `ab -n 5000 -c 50` smoke output;
- `bench-stats-20260504T021828Z.json` — stats after smoke runs.

Smoke results are not capacity claims. They only prove the receiver starts, accepts raw HEC traffic locally, and counters remain clean under small release-mode `ab` runs.

### Phase 2 — Reporting, Error, And Outcome Spine

- Done initial: add `report.rs` with `Reporter`, `ReportContext`, `FactId`, `FactSpec`, typed `FieldValue`, `Outcome`, `Severity`, output routing, and stats/tracing/console adapters.
- Done initial: route existing request counters through Reporter-owned stats sink rather than direct handler counter writes.
- Done initial: add fact registry and typed fields for request, auth, body, parser, and sink paths.
- Done initial: apply `Instant` rule by keeping `Instant` local to request handling and submitting `elapsed_us` to Reporter.
- Done initial: rename the concrete client-visible response type to `HecResponse`.
- Done initial: wire runtime observe config into `tracing-subscriber`, `Reporter` output toggles, and redacted effective config output.
- Continue: move fact constants closer to owning modules once module boundaries are stable.
- Add controlled dynamic diagnostic support for investigation-specific records that still use source, phase, component, severity, redaction, and output routing.
- Avoid a generic `messages.rs` dumping ground. Add `public_text.rs`, `render.rs`, or `output.rs` only if public text and command-output rendering need a separate home after report/outcome types exist.
- Continue: remove stale `HecOutcome` wording from historical notes when those files are edited for other reasons.
- Continue: add `error.rs` classes for config/startup/request/sink beyond current `HecError`.
- Continue: add outcome serialization and mapping tests.
- Continue: add redaction, routing, fan-out, console output, and output-rendering tests beyond current stats and disabled-diagnostic tests.
- Continue: add per-source/component filters after the initial global observe filter proves insufficient.

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
- observe/logging/reporting output toggles are configured by defaults, TOML, CLI, and env;
- handlers no longer scatter HEC response text/code;
- product-significant call sites use domain-owned fact constants and `Reporter::submit(...)` rather than direct backend logging calls;
- rejected and failed outcomes are represented as `Outcome` values, not separate messaging APIs;
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
