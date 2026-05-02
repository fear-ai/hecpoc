# Stack — HECpoc HTTP Stack Without Application-Level Tower

Status: design decision and implementation reference.

Scope: focused Splunk HEC-compatible receiver in Rust, using Tokio and Axum while deliberately avoiding direct use of Tower middleware for protocol-critical behavior. This document covers stack shape, request phases, implementation requirements, fallback options, and validation strategy.

## 1. Decision

Use Tokio and Axum for the first HECpoc implementation, but do not use Tower or `tower-http` middleware in HEC request processing.

This is a precise statement:

- **Keep Tokio** as the async runtime, TCP listener runtime, timers, signal handling, task runtime, and bounded channel implementation.
- **Keep Axum** as the route adapter, request/response adapter, and testable HTTP entry point.
- **Do not add direct dependencies on `tower` or `tower-http`** for HEC auth, gzip, body limits, timeouts, tracing, or backpressure in the first implementation.
- **Do accept Axum's transitive Tower model**. Axum itself is built around `tower::Service`; this cannot be removed while using Axum.
- **Implement protocol-critical behavior explicitly** in our own collector modules.

The practical rule:

```text
Cargo dependency graph may contain tower through Axum.
HECpoc source should not call tower::*, tower_http::*, ServiceBuilder, Layer, or RequireAuthorization.
```

The reason is not anti-Tower ideology. It is that Splunk HEC has visible wire behavior for auth failures, gzip failures, queue pressure, health, ACK state, and malformed payloads. Generic middleware tends to return generic HTTP responses or hide ordering. HECpoc needs transparent, testable phases.

## 2. Can We Drop Tower Entirely?

Not if we use Axum.

Axum's own documentation says Axum integrates with Tower rather than having its own bespoke middleware system, and Axum's `Router` implements `tower::Service`. The current Axum docs also say `axum::serve` is intentionally simple and that Hyper or `hyper-util` should be used when server configuration is needed.

So the options are:

| Option | Direct Tower in our code | Tower in dependency graph | Practical result |
|---|---:|---:|---|
| Axum, no Tower middleware | No | Yes | Recommended initial HECpoc stack |
| Axum + Tower middleware | Yes | Yes | Avoid initially; useful later only for non-protocol concerns |
| Hyper direct + `hyper-util` | No, unless we choose to adapt Tower services | Possibly no | More control, more hand-written routing |
| Tokio TCP + hand-written HTTP | No | No | Not justified; high correctness burden |

Use the first option unless a specific test proves Axum's server path prevents a required behavior.

## 3. Why Avoid Direct Tower Middleware Here?

HEC receiver behavior is not just HTTP plumbing. It is an interoperability surface:

- `Authorization` syntax maps to HEC status codes and JSON response bodies.
- Unsupported or malformed gzip maps to HEC-compatible failure behavior.
- Empty body, malformed JSON, missing `event`, blank `event`, bad `fields`, and queue-full each have specific semantics.
- ACK behavior requires channel state, commit boundaries, and status queries.
- Health is a load-balancer contract, not just "process is alive".
- Body limits and timeouts are security controls and must be visible in tests.

Generic Tower middleware can still be technically correct HTTP, but wrong for HEC. A 401 with an empty body is not a HEC-compatible auth response. A generic 413 may be useful at the socket edge, but it does not by itself answer what Splunk-compatible clients should see or retry.

## 4. HEC Request Pipeline

The initial implementation should use one explicit pipeline for `/services/collector/event` and `/services/collector/raw`, with shared helpers.

```text
accept socket
  -> Axum route match
  -> HEC handler receives Request<Body>
  -> classify endpoint and method
  -> read cheap headers only
  -> phase / health / admission check
  -> authenticate before body collection
  -> reject unsupported encoding before body collection
  -> enforce advertised Content-Length if present
  -> read body with byte cap and timeout
  -> gzip decode if requested, with decompressed cap
  -> parse endpoint-specific payload
  -> build Event records
  -> enqueue/write through bounded Sink path
  -> return HEC JSON outcome
```

Two details are important:

1. The handler must not use Axum's `Bytes` extractor for HEC event/raw endpoints. `Bytes` means Axum has already collected the whole body before auth and body-limit decisions are made.
2. The body reader must enforce both wire-byte and decoded-byte limits. Gzip turns "body size" into two separate resources.

## 5. Proposed Source Layout

The first implementation can stay small:

```text
src/
  main.rs
  hec_receiver/
    mod.rs
    app.rs          # Axum router assembly and State
    handler.rs      # HEC request phase orchestration
    auth.rs         # Authorization parser and token validation
    body.rs         # bounded read, gzip decode, timeout helpers
    outcome.rs      # HEC response codes and JSON response builder
    event.rs        # internal Event shape
    parse_event.rs  # /services/collector/event parsing
    parse_raw.rs    # /services/collector/raw parsing
    sink.rs         # Sink trait only if a second real sink exists; otherwise concrete Capture/File sink
    health.rs       # HEC phase and health response
```

The key design constraint is that `handler.rs` owns the ordering. Helpers should not secretly consume request bodies, mutate response status, or call the sink.

## 6. Tokio Use

Tokio is kept as infrastructure:

- `#[tokio::main]` or explicit runtime for the binary.
- `tokio::net::TcpListener` for binding when using `axum::serve`.
- `tokio::time::timeout` for body-read, decode, enqueue, and shutdown budgets.
- `tokio::sync::mpsc` for bounded ingestion queue when there is a background sink worker.
- `tokio::signal` for graceful shutdown.
- `tokio::task::JoinSet` for owned task groups once there is more than one background task.

Do not use Tokio as a place to hide CPU work. JSON parsing and gzip decompression happen on the request task initially because the PoC body sizes are bounded. If profiling shows gzip or parse dominating the async worker threads, move that specific function behind `spawn_blocking` or a dedicated worker pool.

## 7. Axum Use

Axum should be used narrowly:

- Route matching.
- Shared state extraction.
- Method matching where simple.
- Response conversion.
- HTTP types re-exported through `axum::http`.
- Test harness support by calling the router or handler directly.

Avoid Axum extractors that perform protocol work too early:

- Avoid `Bytes` for HEC body endpoints.
- Avoid `Json<T>` for HEC event bodies because HEC supports concatenated JSON event envelopes, not just one JSON object.
- Avoid generic auth extractors for HEC until our own auth behavior is already proven.

Recommended route shape:

```rust
Router::new()
    .route("/services/collector", post(handle_event))
    .route("/services/collector/event", post(handle_event))
    .route("/services/collector/event/1.0", post(handle_event))
    .route("/services/collector/raw", post(handle_raw))
    .route("/services/collector/raw/1.0", post(handle_raw))
    .route("/services/collector/health", get(handle_health).post(handle_health))
    .route("/services/collector/health/1.0", get(handle_health).post(handle_health))
    .with_state(state)
```

Handlers should take:

```rust
async fn handle_event(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
) -> Response
```

That keeps auth, limits, gzip, body read, and parse order under our control.

## 8. Direct Tower Avoidance

Do not use these in the first HECpoc:

- `tower::ServiceBuilder`
- `tower::limit::*`
- `tower::timeout::*`
- `tower_http::auth::*`
- `tower_http::limit::*`
- `tower_http::timeout::*`
- `tower_http::decompression::*`
- `tower_http::trace::*` for request logging until sensitive-header handling is explicit

This does not ban all future Tower use. It prevents middleware from becoming the protocol. Later, non-protocol middleware may be considered for:

- static file serving if ever added;
- generic HTTP tracing after `Authorization` redaction is tested;
- outer connection/request safety rails that preserve HEC response behavior;
- APIs that are not HEC-compatible wire surfaces.

## 9. Auth Without Tower

### 9.1 Required Behavior

HEC auth must parse `Authorization` without reading the body.

Initial accepted forms:

- `Authorization: Splunk <token>`
- optionally `Authorization: Bearer <token>` for compatibility, if retained from previous spank-rs behavior

Initial rejected forms:

- absent header;
- non-UTF-8 header;
- empty header;
- unsupported scheme;
- supported scheme without token;
- unknown token;
- disabled token later;
- query-string token unless explicitly configured later.

Return HEC JSON responses, not generic HTTP bodies.

### 9.2 Why Not `RequireAuthorization`

Tower's deprecated `RequireAuthorization` / built-in Bearer helper is too small for HEC:

- It is explicitly deprecated in `tower-http` 0.6.8 as "too basic to be useful in real applications".
- It constructs an exact `Bearer <token>` header value.
- It compares the request header to that exact value.
- On failure it returns `401 Unauthorized` with a default/empty body.

That omits HEC's `Splunk` scheme, HEC JSON response body, missing-vs-malformed-vs-invalid-token distinction, token metadata, disabled-token state, index constraints, and token redaction policy.

`AsyncRequireAuthorizationLayer` is more flexible and can attach request extensions, but it still makes auth a Tower layer. For HECpoc, an explicit `auth::authenticate(&HeaderMap, &TokenStore) -> Result<AuthContext, HecError>` is easier to test and reason about.

### 9.3 What Other Projects Do

Observed patterns:

- Vector's HEC source does not use Tower auth. It uses Warp filters and its own `authorization()` filter for HEC-specific behavior. Local source: `/Users/walter/Work/Spank/sOSS/vector/src/sources/splunk_hec/mod.rs`.
- OpenTelemetry Collector's Splunk HEC receiver is Go, not Rust, and handles HEC request behavior in its own handlers rather than delegating to a generic auth middleware. Local source: `/Users/walter/Work/Spank/sOSS/opentelemetry-collector-contrib/receiver/splunkhecreceiver/receiver.go`.
- Axum's own tests still use `ValidateRequestHeaderLayer::bearer("password")` to test routing/layer behavior, but that is not evidence it is suitable for production HEC auth.
- Cargo-registry search showed `AsyncRequireAuthorizationLayer` mostly in `tower-http` examples/tests locally, not as a dominant HEC-style production pattern.

The lesson is not "everyone dropped Tower." It is narrower: serious protocol receivers tend to own their protocol-specific auth rather than using exact-header convenience middleware.

## 10. Gzip Without Tower

### 10.1 Required Behavior

HEC gzip support should:

- inspect `Content-Encoding` before reading the body;
- accept absent encoding as identity;
- accept `gzip` as compressed body;
- decide whether encoding comparison is case-sensitive or case-insensitive and test that choice against Splunk;
- reject unsupported encodings;
- reject malformed gzip;
- enforce decompressed-size limit;
- avoid unbounded allocation;
- avoid leaking decompressor errors into wire responses.

### 10.2 Why Not `tower_http::RequestDecompression`

Tower's request decompression middleware transparently wraps the body based on `Content-Encoding`. For gzip it removes `Content-Encoding` and `Content-Length`, then the handler sees a decompressed body stream. Unsupported encoding returns generic `415 Unsupported Media Type`.

That is useful generic HTTP behavior, but HECpoc needs:

- HEC response bodies and codes;
- explicit compressed and decompressed size accounting;
- explicit malformed-gzip response behavior;
- visible test points for gzip bombs;
- observability that records wire bytes and decoded bytes separately.

Current spank-rs already uses manual `flate2::read::GzDecoder` and maps malformed gzip to HEC invalid data. That is closer to what we need, but HECpoc should improve it by bounding decoded output.

### 10.3 Implementation Shape

```rust
enum Encoding {
    Identity,
    Gzip,
}

fn parse_content_encoding(headers: &HeaderMap) -> Result<Encoding, HecError>;

async fn read_limited_body(
    body: Body,
    max_wire_bytes: usize,
    idle_timeout: Duration,
    total_timeout: Duration,
) -> Result<Bytes, HecError>;

fn decode_body_limited(
    wire: Bytes,
    encoding: Encoding,
    max_decoded_bytes: usize,
) -> Result<Bytes, HecError>;
```

The gzip decoder may start simple with `flate2`, but must write into a bounded buffer. The rule is "stop as soon as the decoded output exceeds the cap," not "decode then check length."

## 11. Body Limits Without Tower

### 11.1 Required Limits

HECpoc needs at least three body-related limits:

| Limit | Applies to | Reason |
|---|---|---|
| `max_content_length` | advertised `Content-Length` | cheap early rejection |
| `max_wire_body_bytes` | actual bytes read from HTTP body | chunked/no-length defense |
| `max_decoded_body_bytes` | decompressed bytes | gzip bomb defense |

A fourth limit may be useful later:

| Limit | Applies to | Reason |
|---|---|---|
| `max_events_per_request` | parsed event count | batch abuse and memory predictability |

### 11.2 Why Not `tower_http::RequestBodyLimit`

Tower's body-limit layer does two useful things:

- It rejects `Content-Length` larger than the configured limit before reading the body.
- It wraps unknown-length bodies so reading past the limit returns an error.

But it returns generic `413 Payload Too Large`, and the limit is one-dimensional. HEC needs a visible policy for wire bytes, decoded bytes, event count, and HEC-compatible responses.

The current spank-rs check happens after Axum has already extracted `Bytes`, which means the whole request has already been read into memory. HECpoc should not repeat that.

### 11.3 Body-Too-Large Code Caveat

Prior local notes have described HEC code `27` as "Request entity too large." Current Splunk troubleshooting documentation for Splunk Enterprise 9.3 lists:

- code `26`: HEC queue at capacity;
- code `27`: HEC ACK channel at capacity.

That contradicts the local note. Treat body-too-large behavior as an open compatibility question:

- verify against local Splunk Enterprise with `Content-Length` over the configured limit;
- capture exact HTTP status, response body, and log behavior;
- do not hard-code code `27` for body-too-large until the local Splunk test confirms it.

For the first PoC, returning HTTP `413` with a HEC-style JSON body may be the clearest internal behavior, but Splunk compatibility must be measured.

## 12. Timeouts Without Tower

### 12.1 Required Timeout Types

HECpoc should define separate budgets:

| Timeout | Scope | Failure class |
|---|---|---|
| accept/shutdown | listener lifecycle | operational |
| header read | Hyper/Axum server path; fallback may require Hyper direct | slowloris |
| body idle | time between body chunks | slow upload |
| body total | total body collection time | resource occupation |
| gzip decode | decompression work | CPU/memory abuse |
| enqueue/write | sink backpressure | server busy |

### 12.2 Why Not `tower_http::TimeoutLayer`

Tower HTTP timeout returns a response with an empty body and configured status. That is not HEC response behavior.

Tower body timeout is inactivity-based and resets after each body frame. That is useful but incomplete: a peer can slowly trickle bytes and never violate the idle timeout. HECpoc needs both idle and total budgets.

### 12.3 Initial Implementation

Use direct Tokio timeouts:

```rust
let body = tokio::time::timeout(total_body_timeout, async {
    read_body_with_idle_timeout(body, max_wire_bytes, idle_timeout).await
}).await;
```

The body reader itself should apply the idle timeout per frame/chunk and a byte cap. The outer timeout caps total duration.

## 13. Backpressure Without Tower

The HEC receiver should not wait indefinitely on downstream storage.

Initial behavior:

- bounded queue or bounded in-memory capture;
- `try_send` or short enqueue timeout;
- if full, return HEC server-busy response;
- count busy responses;
- health should degrade or return unhealthy when queue capacity crosses the configured threshold.

Avoid generic `ConcurrencyLimitLayer` or `LoadShedLayer` initially because they reject at a generic service level. HEC needs queue-aware responses, and queue capacity is part of the HEC health story.

## 14. Request Outcomes

Define HEC outcomes in one place.

Initial outcome fields:

```rust
struct HecOutcome {
    status: StatusCode,
    text: &'static str,
    code: u16,
    metadata: Option<HecOutcomeMetadata>,
}
```

Initial constructors:

- `success()`
- `success_with_ack_id(id)`
- `token_required()`
- `invalid_authorization()`
- `invalid_token()`
- `no_data()`
- `invalid_data_format()`
- `server_busy()`
- `data_channel_missing()`
- `invalid_data_channel()`
- `event_field_required(index)`
- `event_field_blank(index)`
- `ack_disabled()`
- `unsupported_encoding()` if measured Splunk/Vector behavior warrants a non-HEC body, otherwise map to invalid data or HTTP 415 explicitly
- `body_too_large()` with status/body to be verified against Splunk

Keep response text fixed. Do not put exception messages in client-visible bodies.

## 15. Event Endpoint Requirements

The `/services/collector` and `/services/collector/event[/1.0]` handlers should support:

- concatenated JSON event envelopes;
- whitespace between envelopes;
- `event` as string, object, array, number, boolean, depending on measured compatibility target;
- absent `event` -> code `12`;
- `event: null` -> code `12` or compatibility-measured behavior;
- blank string event -> code `13`;
- envelope metadata: `time`, `host`, `source`, `sourcetype`, `index`, `fields`;
- `fields` validation as flat object when supported;
- request-level metadata defaults where Splunk supports query parameters;
- malformed JSON stops processing at first error.

Splunk's current REST reference says malformed event data is processed until an error, then processing stops; successfully processed events before the error may be sent onward. That is a compatibility choice HECpoc must decide deliberately. For a durability-first local store, all-or-nothing request rejection may be cleaner initially, but it is not necessarily Splunk-identical.

## 16. Raw Endpoint Requirements

The `/services/collector/raw[/1.0]` handler should support:

- raw body preservation;
- metadata from query parameters: `host`, `source`, `sourcetype`, `index`, and possibly `time`;
- channel header/query parsing for ACK-enabled mode;
- line splitting policy as an explicit decision, not accidental `split('\n')`;
- CRLF handling tests;
- NUL byte handling tests;
- non-UTF-8 behavior tests;
- empty body -> no data.

Raw endpoint handling should not reuse the JSON parser. It is a separate endpoint with separate framing expectations.

## 17. Health Endpoint Requirements

Health is a load-balancer signal:

- no body required;
- `GET` and possibly `POST` compatibility should be tested;
- return healthy when the receiver can accept new events;
- return unhealthy when queue/backpressure state means HEC should not receive more events;
- later include ACK health once ACK exists.

Current Splunk docs describe `services/collector/health` as checking whether HEC can accept new data and mention queue/ACK capacity. Vector documents HEC source endpoints as `/event`, `/raw`, and `/health`.

## 18. ACK Requirements

ACK is not first implementation unless explicitly selected, but the stack must not block it.

Keep these design hooks:

- parse `X-Splunk-Request-Channel` or `channel` query parameter;
- reject missing channel when ACK is enabled;
- validate channel format when ACK mode requires it;
- reserve response metadata for `ackId`;
- define a commit boundary before implementing ACK;
- keep sink result capable of returning committed IDs or failure.

Do not fake ACK durability. Returning `ackId` before a defined local commit boundary is worse than not supporting ACK.

## 19. Observability and Secret Handling

Without `tower-http::trace`, we must implement small explicit logging:

- request method/path/status/code/duration;
- wire bytes and decoded bytes;
- queue depth and queue full count;
- parse error class, not raw body;
- auth outcome class, not token;
- gzip outcome class;
- timeout class.

Never log the token. If headers are logged for debugging, `Authorization` must be redacted before formatting.

## 20. Alternatives and Fallbacks

### 20.1 Axum With No Direct Tower Middleware

Initial choice.

Benefits:

- minimal application code;
- modern Tokio/Hyper stack;
- route declarations stay readable;
- handlers are testable;
- protocol phases remain explicit.

Risks:

- limited server-level tuning through `axum::serve`;
- underlying Hyper behavior may need investigation for slowloris/header timeouts;
- Axum remains Tower-based internally.

### 20.2 Axum Plus Selected Tower Middleware

Possible later, not first.

Use only if:

- the middleware is outside HEC protocol semantics; or
- tests prove the resulting HEC responses are exactly preserved; and
- direct implementation would be more error-prone.

Examples that may be acceptable later:

- tracing after redaction tests;
- compression for non-HEC responses;
- generic admin API middleware separate from HEC.

### 20.3 Hyper Direct With `hyper-util`

Fallback if Axum prevents necessary server control.

Benefits:

- manual accept loop;
- connection accounting;
- direct use of `hyper-util::server::conn::auto::Builder`;
- `http1_only()` if desired;
- explicit graceful shutdown with watched connections;
- easier insertion of connection-level limits.

Costs:

- hand-written routing;
- hand-written method/path errors;
- more boilerplate around body and response types;
- more burden to preserve behavior across Hyper upgrades.

This fallback should live behind a small adapter:

```text
HTTP adapter -> HEC core
```

The HEC core should not care whether the adapter is Axum or Hyper direct.

### 20.4 Actix-web

Not recommended for HECpoc.

Benefits:

- mature high-performance Rust web framework;
- may perform well in microbenchmarks;
- less Tower exposure.

Costs:

- project-wide runtime and ecosystem shift;
- less direct alignment with the Tokio/Axum/Hyper stack already researched;
- different middleware/extractor model;
- not needed unless Axum/Hyper is proven insufficient.

### 20.5 Warp

Not recommended for a fresh 2026 implementation.

Vector's HEC source uses Warp and is valuable prior art, but Vector also carries an older Hyper 0.14 stack. New HECpoc should borrow protocol findings, not necessarily the framework.

### 20.6 Hand-Written HTTP

Reject.

This would remove Tower entirely but create a new HTTP parser/server correctness project. It is unjustified for HECpoc.

## 21. Validation Strategy

Validation must prove both protocol behavior and stack behavior.

### 21.1 Unit Tests

Target pure functions:

- auth header parser;
- token store lookup;
- content encoding parser;
- bounded gzip decoder;
- HEC outcome JSON serialization;
- event envelope parser;
- raw splitter/framer;
- field validation;
- channel parser;
- time parser.

### 21.2 Handler Tests

Call handlers/router in-process:

- valid event;
- valid raw;
- missing auth;
- malformed auth;
- wrong token;
- unsupported encoding;
- malformed gzip;
- gzip bomb over decoded cap;
- oversized `Content-Length`;
- chunked/no-length body over cap;
- empty body;
- malformed JSON after one valid envelope;
- missing `event`;
- blank `event`;
- object-valued event;
- `fields` nested object;
- queue full.

### 21.3 Local Splunk Oracle

Run the same requests against local Splunk Enterprise HEC first, then HECpoc:

- capture status code;
- capture response body;
- capture whether event is searchable;
- capture Splunk internal log entry if useful;
- record contradictions with current local assumptions.

Especially verify:

- body-too-large response;
- code `27` meaning;
- unsupported `Content-Encoding`;
- `Content-Encoding: GZip` case behavior;
- malformed gzip;
- concatenated JSON partial success behavior;
- raw endpoint channel requirements with ACK disabled/enabled;
- query-string token behavior.

### 21.4 Vector Client Validation

Use Vector as a real HEC client:

- `splunk_hec_logs` sink to Splunk;
- same sink to HECpoc;
- gzip on/off;
- batch sizes varied;
- retries on 503;
- ACK mode later.

Vector's own HEC source is also prior-art code for route, auth, gzip, metadata, and response behavior. Do not treat it as normative when Splunk disagrees; treat it as "production-compatible enough to investigate."

### 21.5 Complex Input Corpora

Use:

- Splunk tutorial logs at `/Users/walter/Work/Spank/Logs/tutorialdata`;
- production Linux/Mac logs under `/Users/walter/Work/Spank/Logs`;
- Wazuh and Vector test/debug logs under `/Users/walter/Work/Spank/Logs`;
- syslog and auth.log samples already used in previous performance work;
- Apache access and error logs;
- generated malformed JSON/gzip/raw fixtures.

For HECpoc, the purpose is not full parsing coverage. The purpose is to prove receiver behavior under real payload sizes, character sets, line endings, and shipper behavior.

### 21.6 Security and Difficult-Scenario Tools

Use dedicated tools where normal curl tests are too polite:

| Tool | Purpose |
|---|---|
| `slowhttptest` | slow headers, slow body, slow read, connection exhaustion |
| `hurl` | repeatable HTTP request/response conformance files |
| `curl` | exact hand-built HEC probes |
| `socat` / `nc` | malformed HTTP, CRLF, early close, odd framing |
| `oha`, `hey`, or `wrk` | request-rate and latency pressure |
| `h2spec` | HTTP/2 conformance if HTTP/2 is enabled |
| `h2load` | HTTP/2 load if HTTP/2 is enabled |
| `cargo-fuzz` | event parser, raw splitter, auth parser, gzip wrapper |
| `cargo-audit` / `cargo-deny` | dependency vulnerability and policy checks |
| `tcpdump` / Wireshark | verify connection close, retransmit, and malformed request behavior |
| custom gzip-bomb generator | decoded-size enforcement |

Slowloris validation should be a release gate for any server-stack change. The test is not "server survives one slow client"; it is "bounded slow clients do not starve valid HEC requests beyond the configured acceptance threshold."

## 22. Reference Findings

### 22.1 Local Code References

- `/Users/walter/Work/Spank/spank-rs/crates/spank-hec/src/receiver.rs` — current Axum HEC receiver; useful but currently extracts `Bytes` before auth and body-limit checks.
- `/Users/walter/Work/Spank/spank-rs/crates/spank-hec/src/processor.rs` — current manual gzip decode and event/raw parsing.
- `/Users/walter/Work/Spank/spank-rs/docs/HECst.md` — local HEC behavior audit, including Vector comparisons and known gaps.
- `/Users/walter/Work/Spank/spank-rs/research/Pyst.md` — Rust stack discussion: Axum/Hyper/Tokio, timeouts, slowloris, body limits.
- `/Users/walter/Work/Spank/spank-rs/docs/Network.md` — prior network stack choices.
- `/Users/walter/Work/Spank/spank-rs/perf/Tools.md` — lab tools, local logs, Splunk/Vector validation setup.
- `/Users/walter/Work/Spank/sOSS/vector/src/sources/splunk_hec/mod.rs` — Vector HEC source implementation: routes, auth filter, gzip filter, metadata defaults, response bodies.
- `/Users/walter/Work/Spank/sOSS/opentelemetry-collector-contrib/receiver/splunkhecreceiver/receiver.go` — OTel HEC receiver handler behavior, gzip reader pool, response constants, health response.

### 22.2 Local Crate Source References

- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tower-http-0.6.8/src/auth/require_authorization.rs` — deprecated Basic/Bearer exact-header auth.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tower-http-0.6.8/src/auth/async_require_authorization.rs` — custom async auth layer; flexible but still Tower-layered.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tower-http-0.6.8/src/limit/mod.rs` — request body limit behavior and smuggling warning.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tower-http-0.6.8/src/decompression/request/service.rs` — transparent request decompression behavior.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tower-http-0.6.8/src/timeout/body.rs` — inactivity timeout reset behavior.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/axum-0.7.9/src/serve.rs` — local Axum `serve` implementation and graceful shutdown wrapper.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hyper-util-0.1.20/src/server/conn/auto/mod.rs` — Hyper connection serving fallback.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hyper-util-0.1.20/src/server/graceful.rs` — direct graceful shutdown utility.

### 22.3 External References

- [Axum crate docs](https://docs.rs/axum/latest) — Axum's Tower integration and Tokio/Hyper compatibility.
- [Axum `serve`](https://docs.rs/axum/latest/axum/fn.serve.html) — documents that `serve` is simple and Hyper/`hyper-util` should be used when configuration is needed.
- [Tower HTTP auth docs](https://docs.rs/tower-http/latest/tower_http/auth/index.html) — current auth module entry point.
- [Tower HTTP deprecated Basic auth](https://docs.rs/tower-http/latest/tower_http/auth/require_authorization/struct.Basic.html) — documents deprecation of the simple auth helper.
- [Tower HTTP body limit docs](https://docs.rs/tower-http/latest/tower_http/limit/index.html) — Content-Length interception and unknown-length body behavior.
- [Tower HTTP decompression docs](https://docs.rs/tower-http/latest/tower_http/decompression/index.html) — transparent decompression behavior.
- [Tower HTTP timeout docs](https://docs.rs/tower-http/latest/tower_http/timeout/index.html) — generic timeout response behavior.
- [Hyper graceful shutdown guide](https://hyper.rs/guides/1/server/graceful-shutdown/) — direct Hyper/`hyper-util` accept-loop shutdown pattern.
- [Splunk HEC setup/use docs](https://docs.splunk.com/Documentation/SplunkCloud/latest/Data/UsetheHTTPEventCollector) — HEC purpose, token auth model, event/raw endpoint role.
- [Splunk HEC format events docs](https://docs.splunk.com/Documentation/Splunk/latest/Data/FormateventsforHTTPEventCollector) — JSON event format and example `Authorization: Splunk <token>`.
- [Splunk input endpoint descriptions](https://help.splunk.com/en/splunk-enterprise/rest-api-reference/9.4/input-endpoints/input-endpoint-descriptions) — `/services/collector`, `/event`, `/raw`, `/health`, ACK behavior, raw endpoint parameters.
- [Splunk HEC troubleshooting/error codes](https://help.splunk.com/en/splunk-enterprise/get-started/get-data-in/9.3/get-data-with-http-event-collector/troubleshoot-http-event-collector) — current HEC status/code table; note code `27` contradiction with local body-limit assumption.
- [Vector Splunk HEC source docs](https://vector.dev/docs/reference/configuration/sources/splunk_hec/) — stable Vector HEC source endpoints and delivery/ACK posture.

## 23. Implementation Checklist

Before writing code:

- confirm first-phase endpoints;
- confirm initial accepted auth schemes;
- confirm initial body limits;
- confirm whether HTTP/2 is enabled or disabled;
- confirm first sink: capture file or in-memory capture;
- confirm whether ACK is explicitly deferred.

While writing code:

- keep handler phase order visible;
- keep protocol helpers pure where possible;
- reject before body read when possible;
- record all HEC outcomes through one type;
- avoid direct Tower imports;
- avoid generic Axum body extractors for HEC body endpoints.

Before accepting code:

- run unit tests for auth, body, gzip, parser, outcomes;
- run handler tests for all negative cases;
- run curl probes against HECpoc;
- run selected probes against Splunk and compare;
- run Vector-to-HECpoc shipment;
- run at least one slow-body or slow-header test before claiming resilience.

## 24. Open Decisions

- Should HECpoc accept `Bearer <token>`, or only Splunk's documented `Splunk <token>` form?
- Should unsupported `Content-Encoding` map to HTTP `415` with plain text, HEC code `6`, or measured Splunk behavior?
- What exact response should body-too-large return?
- Should HTTP/2 be disabled for the first PoC to simplify attack surface and tests?
- Should raw endpoint require channel before ACK exists, or only once ACK mode exists?
- Should event endpoint partial success match Splunk's documented "processed until error" behavior, or should HECpoc require all-or-nothing request acceptance?
- What is the first durable sink: JSONL capture, raw chunk file, SQLite, or append-only segment?

These decisions belong in tests once answered. The Stack decision is only useful if implementation makes the answers executable.
