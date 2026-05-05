# Stack — HECpoc HTTP Stack Without Application-Level Tower

Status: design decision and implementation reference.

Scope: focused Splunk HEC-compatible receiver in Rust, using Tokio and Axum while deliberately avoiding direct use of Tower middleware for protocol-critical behavior. This document records HTTP stack, buffering, and request-processing details that are more specific than the infrastructure-wide design.

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

## 14. Request Responses

Define HEC client-visible responses in one place. Use `Outcome` separately for operation disposition such as accepted, rejected, failed, skipped, throttled, or recovered.

Initial response fields:

```rust
struct HecResponse {
    status: StatusCode,
    text: &'static str,
    code: u16,
    ack_id: Option<u64>,
    invalid_event_number: Option<usize>,
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

## 25. Current Configurability Surface

The infrastructure target for configuration is `/Users/walter/Work/Spank/HECpoc/InfraHEC.md §7`. This section remains as an HTTP-stack ledger for which knobs affect ingress, body handling, buffering, and overload behavior.

Current implementation loads configuration through the `InfraHEC.md §7.2` source chain:

```text
compiled defaults < TOML file < command line < environment
```

### 25.1 Externally Configurable Parameters

| Parameter | TOML key | Environment | Default | Meaning |
| --- | --- | --- | --- | --- |
| Config file | none | `HEC_CONFIG` | none | Optional TOML file path. |
| Listen address | `hec.addr` | `HEC_ADDR` | `127.0.0.1:18088` | Socket address for the receiver. Supports IPv4 or IPv6 socket syntax. |
| Token | `hec.token` | `HEC_TOKEN`, fallback `SPANK_HEC_TOKEN` | `dev-token` | Accepted Splunk HEC token. |
| Capture path | `hec.capture` | `HEC_CAPTURE` | none | Optional JSONL accepted-event capture sink. Absence uses drop sink. |
| Max wire bytes | `limits.max_bytes` | `HEC_MAX_BYTES` | `1_048_576` | Maximum advertised and actually read request bytes before decompression. |
| Max decoded bytes | `limits.max_decoded_bytes` | `HEC_MAX_DECODED_BYTES` | `4_194_304` | Maximum bytes after gzip decompression. |
| Max events | `limits.max_events` | `HEC_MAX_EVENTS` | `100_000` | Maximum HEC events in one request body. |
| Idle body timeout | `limits.idle_timeout` | `HEC_IDLE_TIMEOUT` | `5s` | Maximum time waiting for a body frame. |
| Total body timeout | `limits.total_timeout` | `HEC_TOTAL_TIMEOUT` | `30s` | Maximum wall time to read the request body. |
| Gzip buffer bytes | `limits.gzip_buffer_bytes` | `HEC_GZIP_BUFFER_BYTES` | `8_192` | Scratch buffer used while decoding gzip. |
| Observe level/filter | `observe.level` | `HEC_OBSERVE_LEVEL` | `info` | Global tracing-subscriber filter expression for the current implementation. |
| Observe format | `observe.format` | `HEC_OBSERVE_FORMAT` | `compact` | Tracing output format: `compact` or `json`. |
| Redaction mode | `observe.redaction_mode` | `HEC_OBSERVE_REDACTION_MODE` | `redact` | Redact secrets by default; `passthrough` is explicit debugging override. |
| Redaction text | `observe.redaction_text` | `HEC_OBSERVE_REDACTION_TEXT` | `<redacted>` | Replacement text for redacted values in effective config and later output adapters. |
| Tracing output | `observe.tracing` | `HEC_OBSERVE_TRACING` | `true` | Enables tracing backend output from Reporter fan-out. |
| Console output | `observe.console` | `HEC_OBSERVE_CONSOLE` | `false` | Enables direct console backend output from Reporter fan-out. |
| Stats output | `observe.stats` | `HEC_OBSERVE_STATS` | `true` | Enables stats counter effects from Reporter fan-out. |
| Success code | `protocol.success` | `HEC_SUCCESS` | `0` | HEC success response code. |
| Token required code | `protocol.token_required` | `HEC_TOKEN_REQUIRED` | `2` | Missing auth token. |
| Invalid authorization code | `protocol.invalid_authorization` | `HEC_INVALID_AUTHORIZATION` | `3` | Malformed authorization header. |
| Invalid token code | `protocol.invalid_token` | `HEC_INVALID_TOKEN` | `4` | Unknown token. |
| No data code | `protocol.no_data` | `HEC_NO_DATA` | `5` | Empty request body / no events. |
| Invalid data code | `protocol.invalid_data_format` | `HEC_INVALID_DATA_FORMAT` | `6` | JSON, raw, gzip, or format failure. |
| Server busy code | `protocol.server_busy` | `HEC_SERVER_BUSY` | `9` | Backpressure, sink failure, timeout class. |
| Event missing code | `protocol.event_field_required` | `HEC_EVENT_FIELD_REQUIRED` | `12` | JSON event envelope lacks `event`. |
| Event blank code | `protocol.event_field_blank` | `HEC_EVENT_FIELD_BLANK` | `13` | `event` exists but is blank. |
| Indexed fields code | `protocol.handling_indexed_fields` | `HEC_HANDLING_INDEXED_FIELDS` | `15` | Nested indexed fields rejection. |
| Health code | `protocol.health` | `HEC_HEALTH` | `17` | Health endpoint response code. |

`18194` was not a product default. It was a benchmark-only override used to avoid colliding with the development default `18088` and the browser tab pointed at that default. The real default is still `127.0.0.1:18088`.

### 25.2 Externally Configurable But Without Non-Empty Defaults

- `HEC_CONFIG`: unset means no file config.
- `HEC_CAPTURE`: unset means drop sink; set means accepted events are written JSONL.

### 25.3 Hard-Coded Values Still Present

| Value | Location | Reason to keep for now | Future configuration |
| --- | --- | --- | --- |
| HEC routes | `/services/collector`, `/event`, `/raw`, `/health`, versioned aliases | Protocol compatibility surface, not tuning. | Route enable/disable flags only if product bundles need them. |
| Stats route | `/hec/stats` | Local inspection endpoint. | Operator path or disable flag. |
| Minimum gzip buffer | `1` | Safety guard so zero never creates a non-progressing decode loop. | Keep as invariant, not operator config. |
| Atomic increments by `1` | stats counters | Counter semantics. | Not configurable. |
| Tokio worker count | runtime default | Current `#[tokio::main(flavor = "multi_thread")]` uses Tokio default worker count. | Runtime builder with `worker_threads`, thread names, optional affinity. |
| Listener backlog | OS/Tokio bind default path | Current code uses `TcpListener::bind`, not `TcpSocket::listen(backlog)`. | Add explicit `HEC_LISTEN_BACKLOG`. |
| Socket receive/send buffers | OS default | Current code does not construct sockets through `socket2` or `TcpSocket`. | Add `HEC_TCP_RECV_BUFFER`, `HEC_TCP_SEND_BUFFER`. |
| Keepalive/nodelay/reuseport | OS/default library behavior | Current code does not own per-socket tuning. | Add booleans/options once manual listener setup exists. |

### 25.4 Per-Component Observation Filters

Current implementation has one configured `observe.level` filter applied to `tracing-subscriber`, and Reporter emits fact metadata fields: `phase`, `component`, `step`, `fact`, `request_id`, and typed payload fields. That is enough for compact and JSON output and for post-processing, but not yet enough for efficient runtime per-component filtering.

Why fields alone are insufficient:

- `tracing-subscriber`'s common `EnvFilter` path filters naturally by target/module and level.
- Filtering by arbitrary dynamic fields such as `component="auth"` generally requires a custom `Layer` or post-processing.
- If every event uses target `hec_receiver`, `observe.level = "hec_receiver=debug"` can raise or lower the whole receiver, but not only auth/body/parser/sink.

Preferred next implementation:

```toml
[observe.sources]
hec.auth = "debug"
hec.body = "info"
hec.parser = "warn"
hec.sink = "debug"
```

Reporter maps the fact registry's component to a tracing target:

| Fact component | Tracing target | Example directive |
| --- | --- | --- |
| `Component::Hec` | `hec.receiver` | `hec.receiver=info` |
| `Component::Auth` | `hec.auth` | `hec.auth=debug` |
| `Component::Body` | `hec.body` | `hec.body=info` |
| `Component::Parser` | `hec.parser` | `hec.parser=warn` |
| `Component::Sink` | `hec.sink` | `hec.sink=debug` |

This preserves the intended distinction: component/source is not "the message subsystem"; it is the origin of the reported fact. The Reporter remains the output pipeline, while `Component` and `Step` remain fact metadata owned by the processing design.

Filter examples:

```sh
HEC_OBSERVE_LEVEL='info,hec.auth=debug,hec.sink=debug'
HEC_OBSERVE_LEVEL='warn,hec.body=trace'
HEC_OBSERVE_LEVEL='hec.receiver=info,hec.parser=debug'
```

Fallback if target-level filtering proves too coarse:

- keep target-level source filtering for the hot path;
- add a Reporter-side runtime filter table keyed by `(phase, component, step, fact)` for console, stats, benchmark ledger, and future custom outputs;
- add custom `tracing_subscriber::Layer` field filtering only if real use cases need field-level routing inside tracing itself.

Do not create separate call-site APIs such as `auth_log`, `tcp_log`, or `queue_log`. Product call sites continue to submit facts once; filtering, redaction, and routing remain Reporter/backend behavior.

## 26. Socket and Load Observation Script

Use `/Users/walter/Work/Spank/HECpoc/scripts/capture_net_observe.sh` during benchmark or attack tests. It records timestamps and raw command output into a per-run directory for later processing.

Example:

```sh
cd /Users/walter/Work/Spank/HECpoc
HEC_OBSERVE_PORT=18194 \
HEC_OBSERVE_STATS_URL=http://127.0.0.1:18194/hec/stats \
HEC_OBSERVE_INTERVAL=3 \
HEC_OBSERVE_SAMPLES=120 \
HEC_OBSERVE_OUT=observe/bench-$(date -u +%Y%m%dT%H%M%SZ) \
scripts/capture_net_observe.sh
```

Outputs:

- `manifest.txt`: run metadata and sample timestamps.
- `netstat_states.log`: TCP state bins for the target endpoint.
- `netstat_raw.log`: raw matching TCP rows.
- `lsof_port.log`: process/file descriptor view for the target port.
- `sysctl_network.log`: kernel network knobs visible on macOS.
- `ulimit.log`: process resource limits.
- `stats.log`: receiver `/hec/stats` snapshots.
- `stats.pretty.jsonl`: pretty JSON copies when `jq` is installed.

The script is intentionally plain shell. It should remain easy to run on a lab host before we introduce a richer Rust benchmark harness.

## 27. Benchmark and Hostile Input Repositories

Cloned under `/Users/walter/Work/Spank/sOSS`:

| Repo | Local path | Use |
| --- | --- | --- |
| Apache HTTPD / ApacheBench | `/Users/walter/Work/Spank/sOSS/apache-httpd` | Source for `ab` and `apr_socket_connect` behavior. |
| `wrk` | `/Users/walter/Work/Spank/sOSS/wrk` | High-throughput C/Lua HTTP benchmark. |
| `oha` | `/Users/walter/Work/Spank/sOSS/oha` | Rust/Tokio load generator with JSON output and modern options. |
| `bombardier` | `/Users/walter/Work/Spank/sOSS/bombardier` | Go load generator, useful cross-check against `ab` and `oha`. |
| `vegeta` | `/Users/walter/Work/Spank/sOSS/vegeta` | Rate-controlled load and replay style testing. |
| `hey` | `/Users/walter/Work/Spank/sOSS/hey` | Simple Go load tool; useful baseline. |
| NGINX | `/Users/walter/Work/Spank/sOSS/nginx` | Mature connection accounting, accept balancing, idle culling reference. |
| Pingora | `/Users/walter/Work/Spank/sOSS/pingora` | Rust high-performance proxy framework with connection pooling and graceful reload ideas. |
| Linkerd proxy | `/Users/walter/Work/Spank/sOSS/linkerd2-proxy` | Rust/Tokio/Hyper/Tower production proxy reference. |
| Pingap | `/Users/walter/Work/Spank/sOSS/pingap` | NGINX-like Rust reverse proxy built on Pingora. |
| PayloadsAllTheThings | `/Users/walter/Work/Spank/sOSS/PayloadsAllTheThings` | Attack payload patterns for Log4Shell, path traversal, injection, encodings. |
| SecLists sparse checkout | `/Users/walter/Work/Spank/sOSS/SecLists` | Fuzzing payloads and zip-bomb payload families without full repository bulk. |
| slowhttptest | `/Users/walter/Work/Spank/sOSS/slowhttptest` | Slow headers, slow body, slow read, and range-style DoS tooling. |
| Radamsa | `/Users/walter/Work/Spank/sOSS/radamsa` | Mutation fuzzing from valid HEC and log samples. |
| LogHub | `/Users/walter/Work/Spank/sOSS/LogHub` | Public structured log corpus: Apache, OpenSSH, syslog-like families. |
| Splunk Attack Data | `/Users/walter/Work/Spank/sOSS/splunk-attack-data` | Splunk-oriented attack logs and replay corpus. |

Local validation data remains primary for ingest behavior:

- `/Users/walter/Work/Spank/Logs/tutorialdata`
- `/Users/walter/Work/Spank/Logs/spLogs/laz24_20260310_233030/syslog`
- `/Users/walter/Work/Spank/Logs/spLogs/laz24_20260310_233030/auth.log`
- `/Users/walter/Work/Spank/Logs/loghub/Apache_2k.log`
- `/Users/walter/Work/Spank/Logs` broader production, Vector, Wazuh, debug, Linux, and Mac logs.

## 28. Current Accept and Receive Processing

### 28.1 Current HEC Receiver

`/Users/walter/Work/Spank/HECpoc/src/main.rs` currently binds with `tokio::net::TcpListener::bind(addr).await?` and then calls `axum::serve(listener, app)`. That means the application does not own the explicit accept loop today. It owns HEC request phases after Axum/Hyper have accepted a connection and produced a request.

`/Users/walter/Work/Spank/HECpoc/src/hec_receiver/handler.rs` owns the application receive pipeline:

1. health admission;
2. auth header parse and token check;
3. content encoding parse;
4. advertised `Content-Length` rejection;
5. bounded body frame read with idle and total timeouts;
6. bounded gzip decode when requested;
7. event/raw parse;
8. sink submit;
9. stats update and HEC response.

### 28.2 Tokio Accept Path

Tokio `TcpListener::accept` uses readiness and the underlying nonblocking socket accept:

- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.52.1/src/net/tcp/listener.rs:163` — async `accept` waits for readable readiness and calls `self.io.accept()`.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.52.1/src/net/tcp/listener.rs:180` — `poll_accept` loops on readiness and clears readiness on `WouldBlock`.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.52.1/src/net/tcp/socket.rs:902` — `TcpSocket::listen(backlog)` exposes explicit listen backlog when we stop using `TcpListener::bind`.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.52.1/src/net/tcp/socket.rs:385` — `set_recv_buffer_size` exposes `SO_RCVBUF`.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.52.1/src/net/tcp/socket.rs:229` — `set_reuseaddr` is available.

### 28.3 Axum Accept Path

Axum `serve` is deliberately simple:

- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/axum-0.8.9/src/serve/mod.rs:189` — run loop calls `listener.accept().await` and passes the IO to `handle_connection`.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/axum-0.8.9/src/serve/listener.rs:30` — TCP listener implementation retries accept after recoverable errors.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/axum-0.8.9/src/serve/listener.rs:140` — non-connection accept errors are logged and slept for one second; comment notes Axum does not expose the old Hyper customization knob.

### 28.4 Hyper Body Receive Path

Hyper turns HTTP body bytes into an `Incoming` body:

- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hyper-1.9.0/src/body/incoming.rs:52` — `Incoming` body holds either empty, HTTP/1 channel, HTTP/2 stream, or FFI body.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hyper-1.9.0/src/body/incoming.rs:115` — HTTP/1 bodies use an internal zero-capacity channel, which coordinates producer/consumer backpressure.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hyper-1.9.0/src/body/incoming.rs:243` — HTTP/2 body polling uses `poll_data` and releases flow-control capacity after bytes are consumed.
- `/Users/walter/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hyper-1.9.0/src/proto/h1/decode.rs:483` — chunked HTTP/1 body reads are decoded stepwise.

## 29. NGINX and Rust Proxy Connection Lessons

### 29.1 NGINX Connection Tracking

NGINX preallocates connection objects per worker and maintains free/reusable lists rather than allocating arbitrary per-connection state under pressure.

Important local references:

- `/Users/walter/Work/Spank/sOSS/nginx/src/event/ngx_event.c:123` — `worker_connections` directive.
- `/Users/walter/Work/Spank/sOSS/nginx/src/event/ngx_event.c:455` — logs when worker connections are not enough.
- `/Users/walter/Work/Spank/sOSS/nginx/src/core/ngx_connection.c:1207` — `ngx_get_connection` pulls from the free connection list.
- `/Users/walter/Work/Spank/sOSS/nginx/src/core/ngx_connection.c:1273` — `ngx_free_connection` returns connection state to the free list.
- `/Users/walter/Work/Spank/sOSS/nginx/src/core/ngx_connection.c:1381` and `:1395` — reusable connection count is decremented/incremented as idle reuse state changes.
- `/Users/walter/Work/Spank/sOSS/nginx/src/core/ngx_connection.c:1427` — idle culling chooses up to `max(min(32, reusable / 8), 1)` reusable connections.
- `/Users/walter/Work/Spank/sOSS/nginx/src/core/ngx_connection.c:1462` — close idle connections path.
- `/Users/walter/Work/Spank/sOSS/nginx/src/event/ngx_event_accept.c:345` — accept mutex path used to level accept work across workers.

For HEC, the immediate lesson is not to copy NGINX literally. The useful shape is bounded state: pre-sized tables or slab indices, explicit reusable/idle queues, counters that reconcile arithmetically, and culling policy independent of parser code.

### 29.2 Rust Implementations With Similar Goals

- Pingora: Rust async multithreaded proxy framework. Locally useful files include `/Users/walter/Work/Spank/sOSS/pingora/pingora-pool/src/connection.rs`, which uses keyed connection pools, hot queues, maps, idle watchers, and explicit eviction/timeout paths. It is closest to an NGINX successor in ambition, but as a framework, not a drop-in web server.
- Linkerd proxy: Rust production service-mesh proxy. It is more Tower/Hyper/service-composition oriented than HEC needs, but useful for disciplined readiness, backpressure, and per-connection observability patterns.
- Pingap: NGINX-like reverse proxy built on Pingora; useful to see how Pingora is productized into operator configuration.
- Vector and OTel collector: not NGINX competitors, but directly relevant ingest systems; they show HEC compatibility and sink/backpressure tradeoffs.

### 29.3 Candidate Data Structures For HEC Connection Records

Initial in-process structures should be boring and bounded:

```text
ConnectionRegistry
  next_id: AtomicU64
  current: AtomicU64
  accepted_total: AtomicU64
  closed_total: AtomicU64
  rejected_total: AtomicU64
  max_current: AtomicU64
  by_id: DashMap<ConnectionId, Arc<ConnectionRecord>> or sharded RwLock<Vec<Option<Record>>>
  by_ip: DashMap<IpKey, IpCounters>
  idle_heap: BinaryHeap<Reverse<(last_io_ns, ConnectionId)>>
  recent_rings: sharded fixed-size second buckets
```

`DashMap` is convenient but not free. For very large counts, prefer sharded vectors/slabs:

- `slab` or `slotmap` for stable IDs with compact storage;
- per-worker `Vec<Option<ConnectionRecord>>` to avoid global write locks;
- `crossbeam_queue::SegQueue` or per-shard free lists for reusable slots;
- `BinaryHeap` or timing wheel for culling by idle time;
- `BTreeMap` only for small sorted admin views, not hot updates;
- approximate top-N sketches for heavy talkers if exact sorting becomes expensive.

Connection sorting/binning views should be derived snapshots, not maintained in every hot-path update:

- by start time;
- by last I/O time;
- by total wire bytes;
- by lifetime bytes/sec;
- by recent bytes/sec;
- by request count;
- by source IP or CIDR range;
- by token/source/type once metadata exists.

IPv4 and IPv6 must use one normalized key type:

```text
IpKey = IpAddr plus prefix length
IPv4 exact: 192.0.2.10/32
IPv4 range: 192.0.2.0/24
IPv6 exact: 2001:db8::1/128
IPv6 range: 2001:db8::/64
```

Do not string-key hot-path IP data. Store `IpAddr`/integer representation and format only for reporting.

### 29.4 Connection Counter Arithmetic

Use these names:

- `connections_current`
- `connections_accepted_total`
- `connections_closed_total`
- `connections_rejected_total`
- `connections_max`

Arithmetic invariants:

```text
connections_current = connections_accepted_total - connections_closed_total
```

Rejected connection attempts were never admitted, so they do not enter `connections_current` and do not require a later close:

```text
connection_attempts_total = connections_accepted_total + connections_rejected_total
```

`connections_max` is a high-water mark:

```text
connections_max = max(previous connections_max, connections_current after accept)
```

If the accept loop cannot accept because the OS has already rejected or queued beyond visibility, that is not counted unless the application observes it. Application-visible reject reasons should be separate counters: `reject_limit_global`, `reject_limit_ip`, `reject_shutdown`, `reject_auth_pressure`, `reject_queue_full`.

## 30. Kernel, Runtime, and Library Knobs

### 30.1 macOS Visibility And Knobs

Current useful commands:

```sh
sysctl kern.ipc.somaxconn net.inet.ip.portrange.first net.inet.ip.portrange.last net.inet.tcp.msl
netstat -anv -p tcp
lsof -nP -iTCP:<port>
ulimit -n
```

Relevant meanings:

- `kern.ipc.somaxconn`: upper influence on listen backlog.
- `net.inet.ip.portrange.first/last`: client-side ephemeral port range; localhost load generators can hit this first.
- `net.inet.tcp.msl`: TIME_WAIT lifetime factor; affects churn tests.
- `ulimit -n`: process file descriptor limit.

### 30.2 Linux Visibility And Knobs

Useful commands:

```sh
ss -tan state established '( sport = :18194 or dport = :18194 )'
ss -s
cat /proc/net/sockstat
cat /proc/net/netstat
sysctl net.core.somaxconn net.ipv4.ip_local_port_range net.ipv4.tcp_fin_timeout net.ipv4.tcp_tw_reuse
sysctl net.core.rmem_default net.core.rmem_max net.core.wmem_default net.core.wmem_max
sysctl net.ipv4.tcp_rmem net.ipv4.tcp_wmem net.ipv4.tcp_max_syn_backlog
ulimit -n
```

Add perf/cpu visibility when benchmarking on Linux:

```sh
pidstat -t -p <pid> 1
perf stat -p <pid> -d -- sleep 30
perf top -p <pid>
numactl --hardware
lscpu --extended
```

### 30.3 Tokio And Companion Capabilities

Current code uses the simple path. Expansion path:

- `tokio::net::TcpSocket`: explicit IPv4/IPv6 socket creation, `set_reuseaddr`, `set_recv_buffer_size`, `listen(backlog)`.
- `socket2`: access to options Tokio does not expose directly, including platform-specific keepalive, reuseport, send buffer, dual-stack controls, and raw file-descriptor/socket conversion.
- Manual `listener.accept().await`: connection registry, per-IP admission, high-water stats, and culling hooks.
- `hyper-util`: direct per-connection serving once Axum `serve` is too opaque.
- Tokio runtime builder: explicit worker threads and thread names; CPU affinity requires OS calls or a crate such as `core_affinity` and careful measurement.

Forking Tokio/Axum/Hyper is not justified for current HEC work. The needed controls are available through public APIs once we own listener construction and connection serving.

## 31. Ingest Resilience And DoS Policy Surface

Promoted HEC ingest behavior should be explicit: bounded bytes, bounded decoded bytes, bounded events, bounded time, bounded queue, bounded connection count, bounded per-IP contribution, visible outcomes, no crash on malformed input.

Policy must include both values and actions. Suggested policy dimensions:

| Condition | Configurable value | Action options | Default first choice |
| --- | --- | --- | --- |
| Global connection limit | max current connections | reject, close oldest idle, close newest, degrade health | reject new and count. |
| Per-IP connection limit | max current per exact IP/range | reject, close newest, allow but mark suspicious | reject new. |
| Accept backlog pressure | backlog, admission mode | reject, shed by IP, stop accepting briefly | reject where visible; record accept errors. |
| Header/auth malformed | header size/count if exposed, auth policy | 401/403, close after response, immediate close | HEC JSON response where possible. |
| Body advertised too large | max bytes | reject before read, close, drain then close | reject before read. |
| Body grows too large | max bytes | stop reading and respond, close, drain limited | stop reading and respond. |
| Gzip decoded too large | max decoded bytes | reject whole request, close, report | reject whole request. |
| Gzip malformed | none/format policy | reject, close, report | reject with invalid data. |
| Slow body | idle and total timeout | 408, close, temporary IP penalty | timeout and close after response if possible. |
| Too many events | max events | reject whole request, accept prefix, split batch | reject whole request initially. |
| Malformed later event | parser policy | reject whole request, accept prefix, skip invalid | reject whole request until Splunk oracle tests demand otherwise. |
| Queue full | queue depth and enqueue timeout | 503 busy, block briefly, drop, spill to disk | 503 busy; no silent drop. |
| Sink failure | sink retry policy | 503, retry bounded, spill, drop with counter | 503 for request-scoped failure. |
| Parser optional fields | schema policy | preserve raw only, parse shallow, parse deep later | preserve raw plus shallow HEC fields. |

These policies should be represented as data, not hidden in routine names such as `drop_only`. Names such as `SinkMode::Drop`, `SinkMode::CaptureFile`, `AdmissionPolicy`, `OverflowAction`, and `MalformedEventPolicy` are preferable because the source, token, thread, or flow can carry flags for later interpretation.

## 32. Text Processing After Auth And Decompression

Before storage optimization, define the text pipeline as observable stages with test groups.

### 32.1 Stream And Buffer Stages

1. HTTP stream: frame-by-frame read with wire byte accounting.
2. Decode stream: identity or gzip with decoded byte accounting.
3. Boundary stream: raw endpoint line splitter or event endpoint concatenated JSON object stream.
4. Batch builder: converts decoded bytes into `IngestBatch` with source metadata.
5. Queue routing: maps context to a queue; initially always queue `0`.
6. Worker processing: assigns sequence IDs and writes minimal durable records.

Context passed into queue mapping should include:

- session ID / connection ID;
- host or peer IP;
- fd/socket identifier where available;
- Tokio worker/thread ID when available;
- endpoint;
- file path for file watcher sources;
- extension and declared type for file inputs;
- token/source class;
- decoded format guess;
- source flags and policy flags.

Initial mapper:

```text
map_to_queue(context) -> QueueId(0)
```

The important part is preserving the signature now so later sharding by host, source, token, file, CPU, or load does not rewrite the whole ingest path.

### 32.2 Lines

Tests:

- LF, CRLF, final line without newline;
- empty lines and whitespace-only lines;
- NUL inside line;
- invalid UTF-8;
- very long line below/above configured cap;
- mixed syslog/auth/apache samples;
- line count above `HEC_MAX_EVENTS`.

Policy choices:

- preserve raw bytes and lossy text separately if binary validity matters;
- for now, raw endpoint stores lossy-decoded string and must not panic;
- optional later: byte-preserving raw store plus decoded display column.

### 32.3 Events

Tests:

- one JSON envelope;
- concatenated envelopes;
- whitespace between envelopes;
- missing `event`;
- null/blank event;
- nested indexed fields;
- malformed second envelope;
- huge field object;
- metadata carry-forward if/when Splunk compatibility requires it.

Initial behavior: parse all-or-nothing, preserve original raw value of `event`, store shallow metadata, reject on first invalid envelope with `invalid-event-number` when known.

### 32.4 Fields And Values

Stage shallow fields first:

- `_time` / HEC `time`;
- `_raw` / raw event string;
- `host`;
- `source`;
- `sourcetype`;
- `index`;
- endpoint;
- token/source class;
- ingest time;
- parser version.

Tests:

- absent vs null vs empty string;
- integer/float/string time;
- unknown fields preserved or ignored according to policy;
- large values;
- Unicode and invalid UTF-8 path from raw endpoint;
- field name collisions between HEC metadata and parser-derived fields.

### 32.5 Tokenization And Indexing

Tokenization and deeper parsing are not request-path requirements for first HEC acceptance. They should be replayable from durable raw/event records.

Future stages:

- breaker/tokenizer per sourcetype;
- lowercase/fold policy;
- position-aware token index;
- field-aware token index;
- Sigma-oriented token and field acceleration;
- optional regex extraction;
- per-format parser packs for syslog, auth.log, Apache access, Apache error.

## 33. Why Not Per Host, File, Or Log-Type Databases During Ingest

Per-host, per-file, or per-log-type databases sound attractive because they make ownership obvious. They are bad default ingest partitions because they multiply handles, schemas, compaction work, query fanout, and migration surfaces before the workload has proven stable.

Use hot buckets instead:

- append to current writable bucket by time/size;
- write core metadata columns for host/source/sourcetype/index/file;
- seal buckets into immutable units;
- build per-field/per-token sidecar indexes against sealed buckets;
- resort/coalesce later for search if measurements justify it.

Arithmetic reason: if there are `H` hosts, `T` log types, and `F` files, per-combination ingest storage can create up to `H*T*F` active targets. Buckets bound active targets by writer count and time/size policy. Search can still prune by host/type using metadata and indexes without making ingest pay the full Cartesian product.

For one huge file or one huge log type, a dedicated parser/index worker may be justified, but that should be queue routing or bucket metadata, not a separate database identity. The system should be able to coalesce or repartition sealed data after ingest once event counts, field cardinalities, and query selectivity are known.

## 34. Follow-Up Task Register

### 34.1 Immediate Code Controls

| Priority | Task | Outcome |
| --- | --- | --- |
| P0 | Add `ConnectionStats` counters | True connection current/accepted/closed/rejected/max visibility. |
| P0 | Introduce manual listener construction | Explicit backlog, recv buffer, IPv4/IPv6 choice, socket policy. |
| P0 | Add bounded enqueue/dequeue | Separate request acceptance from sink/index work with queue-full behavior. |
| P1 | Add policy structs | Values plus actions for body, gzip, parser, queue, connection limits. |
| P1 | Add source context struct | Carries connection/session/file/source metadata to queue mapper. |
| P1 | Add `/hec/connections` snapshot | Sorted/bin views for current connections. |

### 34.2 Validation And Benchmarks

| Priority | Task | Outcome |
| --- | --- | --- |
| P0 | Run `capture_net_observe.sh` alongside `ab`, `oha`, and `wrk` | Correlate receiver stats with kernel/client limits. |
| P0 | Verify `apr_socket_connect` failure with port/TIME_WAIT state | Distinguish client exhaustion from server refusal. |
| P1 | Add hostile input corpus harness | Replay PayloadsAllTheThings, SecLists, Radamsa mutations, slowhttptest cases. |
| P1 | Add Splunk oracle comparisons | Confirm codes for body-too-large, late-invalid event, gzip errors. |
| P2 | Add Linux benchmark host pass | Validate Linux-specific knobs, affinity, receive buffers, and perf counters. |

### 34.3 Storage And Processing

| Priority | Task | Outcome |
| --- | --- | --- |
| P0 | Define `IngestBatch` and `SourceContext` | Stable enqueue boundary. |
| P0 | Implement one queue and one worker | Prove state transitions and counters. |
| P1 | Add hot bucket JSONL or binary segment | Durable local store without schema explosion. |
| P1 | Add replayable parsing workers | Keep deeper parsing off request path. |
| P2 | Add token/field index prototype | Sigma/search acceleration after raw durability. |

### 34.4 Documentation

| Priority | Task | Outcome |
| --- | --- | --- |
| P0 | Keep Stack.md as implementation stack ledger | Networking, HTTP, DoS, concurrency, tools. |
| P1 | Split benchmark recipes once repeated | Move commands/results to `Bench.md` if Stack grows too wide. |
| P1 | Split policy spec once implemented | Move policy tables to `Policy.md` after code names settle. |
| P2 | Add source corpus manifest | Stable list of local/public logs and attack corpora. |

## 35. Backpressure And Buffering From Network To Store

Backpressure is not one queue. It is a sequence of queues, buffers, admission choices, and retry/drop policies from the sender's network stack to our sink. HECpoc should name each layer so performance problems are not misattributed to Tokio, Axum, SQLite, or the parser without evidence.

### 35.1 Layered Path

```text
client app
  -> client userspace buffer
  -> client TCP send buffer
  -> network / loopback path
  -> server SYN backlog / accept queue
  -> server accepted socket
  -> server TCP receive buffer
  -> Tokio readiness
  -> Hyper HTTP parser/body channel
  -> HEC body read buffer
  -> optional gzip decode buffer
  -> raw line splitter or JSON envelope parser
  -> EventBatch allocation
  -> bounded ingest queue
  -> sink worker input buffer
  -> file BufWriter / DB transaction / index update buffers
  -> kernel page cache / storage device queue
```

A slowdown at any layer can look like "HEC is slow" unless the layer has counters or sampled visibility.

### 35.2 IP And TCP Admission

Before application code sees data, the kernel controls connection attempts and byte flow:

- SYN backlog and accept queue determine whether connection setup succeeds under bursts.
- Ephemeral port range and `TIME_WAIT` determine how far localhost clients such as `ab` can push before client-side `EADDRNOTAVAIL`.
- TCP receive buffer determines how much data can sit per accepted socket before application reads.
- TCP flow control naturally slows senders when receive buffers fill.
- TCP sequence numbers and ACK windows preserve byte order; HEC raw/event parsing can assume ordered bytes per connection, not ordered requests across connections.
- Packet loss, ECN, RED, CoDel, fq_codel, or NIC queue policies may affect remote tests but are mostly absent in loopback tests.

RED-style early drop matters conceptually: shedding before buffers are saturated can keep latency bounded. In HEC terms, application equivalents are per-IP admission limits, queue high-water health degradation, and `503 Server is busy` before memory is exhausted.

### 35.3 Tokio, Hyper, And Axum Buffers

Current HECpoc does not own the accept loop. Axum accepts and Hyper parses HTTP before the handler runs. The handler then reads body frames into a bounded `Vec`.

Implications:

- `HEC_MAX_BYTES` bounds what the handler accumulates, not necessarily all bytes Hyper or the kernel may have already buffered.
- `Content-Length` rejection avoids reading known-oversized bodies, but chunked or missing-length bodies still require reading until the configured cap is reached.
- Hyper's HTTP/1 body path uses a channel between protocol parser and body consumer, so not polling the body can apply backpressure inside Hyper.
- Owning the listener through `TcpSocket` enables backlog and socket buffer settings; direct Hyper/hyper-util serving enables true per-connection accounting and culling.

### 35.4 HEC Request Buffers

Current buffers:

| Buffer | Current status | Risk | Desired control |
| --- | --- | --- | --- |
| Wire body `Vec` | bounded by `HEC_MAX_BYTES` | one large allocation per large request | keep bounded; maybe preallocate from content length up to cap |
| Gzip output `Vec` | bounded by `HEC_MAX_DECODED_BYTES` | expansion bomb pressure | keep stop-on-cap behavior |
| Gzip scratch | `HEC_GZIP_BUFFER_BYTES` | too small wastes calls; too large wastes per decode | keep configurable |
| Raw line iterator | scalar `split` over decoded body | acceptable for bounded body; slower than `memchr` for large raw batches | switch to `memchr` once raw throughput matters |
| JSON event vector | bounded by `HEC_MAX_EVENTS` | many small events allocate many `HecEvent`s | queue `EventBatch`; later use chunked/vectorized batch storage |
| File sink writes | async file open/write/flush per batch today | sink can dominate and serialize requests | move behind queue worker and explicit flush policy |

### 35.5 File And DB Sink Backpressure

File and DB sinks should not run directly inside request handlers once correctness smoke tests are over.

File path:

```text
request handler -> EventBatch -> bounded queue -> file worker -> BufWriter -> flush policy -> optional fsync policy
```

DB path:

```text
request handler -> EventBatch -> bounded queue -> DB worker -> transaction batch -> commit policy -> checkpoint policy
```

Important distinctions:

- `accepted`: HTTP request parsed and valid.
- `queued`: batch entered the bounded queue.
- `captured`: sink wrote bytes to userspace writer or async file call returned.
- `flushed`: userspace writer flushed to kernel.
- `durable`: fsync or DB durable commit completed.

HEC success must mean exactly one of these states per sink mode. Until ACK exists, do not imply durable commit unless success waits for durability.

### 35.6 Options Through The System

| Layer | Knob | First behavior | Later options |
| --- | --- | --- | --- |
| Accept queue | backlog | OS default | configured `listen(backlog)` |
| Socket | recv/send buffers | OS default | `TcpSocket`/`socket2` configured sizes |
| Per-source admission | global/per-IP limits | none | reject/close/cull policies |
| HTTP headers | header size/time | Hyper defaults | direct Hyper config or custom accept/read path |
| Wire body | max bytes/time | bounded handler read | preallocation and drain/close choices |
| Decode | max decoded bytes | bounded gzip decode | gzip reader pool, worker offload if measured |
| Raw lines | splitter | scalar split | `memchr`, streaming splitter, max line bytes |
| Event batch | max events | bounded vector | chunked batches, prefix-accept policy if required |
| Queue | depth/enqueue timeout | not yet separated | try_send, bounded wait, spill, shed |
| Sink | file/DB buffers | capture file or drop | worker pool, transaction sizing, flush/fsync policy |
| Store/search prep | hot buckets/indexes | deferred | replay workers, sealed-bucket indexing |

## 36. Raw Line Splitting: Why It Is Still Naive

Current raw parsing uses byte split on newline in `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/parse_raw.rs`. That is intentionally naive for the current phase, not a final performance claim.

Reasons it remains acceptable right now:

- The body is already bounded by `HEC_MAX_BYTES` and `HEC_MAX_DECODED_BYTES`; it is not an unbounded stream parser yet.
- Raw handling is still validating endpoint semantics: CRLF trimming, blank lines, NUL, invalid UTF-8, no-data behavior.
- The code is easy to inspect and fuzz; correctness failures are obvious.
- Current bottleneck is more likely HTTP/body/sink policy than delimiter scanning for small-to-medium request bodies.
- SpankMax already proved the direction for optimized delimiter scanning: `memchr` line iteration in `/Users/walter/Work/Spank/spank-rs/perf/src/parsers.rs`.

When to replace it:

- raw endpoint benchmarks show delimiter scanning as material CPU;
- request bodies regularly carry many thousands of lines;
- line max bytes must be enforced without building all events first;
- streaming body parsing replaces whole-body buffering;
- tokenization/index construction starts sharing the same line boundaries.

Replacement path:

1. keep current scalar splitter as reference behavior;
2. add `memchr` dependency;
3. implement `LineSplitter` with `Scalar` and `Memchr` variants;
4. run scalar/memchr agreement tests over local logs and hostile inputs;
5. default to `memchr` only after behavior and benchmark agreement;
6. later add streaming splitter that consumes body frames without requiring one decoded body allocation.

This preserves the PerfIntake principle: keep a scalar correctness path before optimized paths, use proven crates before unsafe/SIMD, and import SpankMax ideas only when the benchmark names the exact need.

### 36.1 Byte And Character Semantics By Stage

The splitter should be treated as a byte-framing component, not as the whole text interpretation policy.

| Stage | What it sees | CRLF | NUL | Control bytes | Non-ASCII / invalid UTF-8 |
| --- | --- | --- | --- | --- | --- |
| TCP/kernel/Tokio/Hyper body | bytes | no special meaning before HEC parsing | data | data | data |
| HTTP headers | header syntax and `HeaderValue` | invalid or normalized by HTTP layer depending location | invalid for normal header text | generally invalid for auth/content headers | `to_str()` rejects non-visible/non-ASCII header text |
| gzip decode | compressed and decoded bytes | data after decode | data after decode | data after decode | data after decode |
| raw line splitter | decoded body bytes | split on LF and trim one preceding CR | data inside line | LF is delimiter, CR only trimmed before LF, others are data unless policy rejects | currently converted with `String::from_utf8_lossy`; future store should keep bytes plus derived text |
| JSON event parser | JSON UTF-8 text | valid inside JSON strings only when escaped or ordinary whitespace | invalid unless escaped as `\u0000` | unescaped controls invalid JSON | JSON must be valid UTF-8 |
| tokenization/indexing | raw bytes or decoded field text by policy | tokenizer-specific boundary | tokenizer-specific term or rejected byte | tokenizer-specific normalize/reject/drop | tokenizer-specific normalize, preserve, or reject |

Current raw behavior should therefore be documented as:

- split only on byte LF `0x0a`;
- remove one trailing CR `0x0d` from a line produced by LF splitting;
- preserve embedded CR, NUL, other control bytes, and non-ASCII until a later policy says otherwise;
- avoid panics by using lossy UTF-8 conversion for current `HecEvent.event`;
- mark lossy conversion as a temporary event-model compromise, not a byte-exact storage design.

Required splitter tests:

- LF, CRLF, lone CR, embedded CR, and final line without LF;
- empty line, whitespace-only line, and no-data body;
- embedded NUL and ASCII controls other than CR/LF;
- valid UTF-8 multibyte text;
- invalid UTF-8 byte sequences;
- very long line at, below, and above the configured line limit.

## 37. Aliases Versus Canonical Internals

Compatibility aliases should be accepted in parallel with internal canonical decomposition.

External aliases:

- `/services/collector` -> canonical `event` endpoint;
- `/services/collector/event` -> canonical `event` endpoint;
- `/services/collector/event/1.0` -> canonical `event` endpoint;
- `/services/collector/raw` -> canonical `raw` endpoint;
- `/services/collector/raw/1.0` -> canonical `raw` endpoint;
- `/services/collector/health` and `/services/collector/health/1.0` -> canonical `health` endpoint.

Internal names should remain stable:

```text
EndpointKind::{Event, Raw, Health, Ack}
HecRequest
SourceContext
EventBatch
EventSink
SinkCommit
HecResponse
```

Do not create separate handlers, parser types, sink paths, or metrics labels for each alias unless testing alias behavior itself. Route alias is metadata; endpoint kind is behavior.

## 38. PerfIntake Perspective Applied

PerfIntake argued against importing `spank-rs/perf` wholesale and for distilling four things: HEC acceptance tests, feature rows, validation lab procedure, and sink/parser/benchmark lessons. Applied to current implementation:

- `spank-rs/perf/src/parsers.rs` informs raw/log parser optimization, not first HEC body acceptance.
- `spank-rs/perf/src/normalize.rs` informs future columnar batches, not current `HecEvent` shape.
- `spank-rs/perf/src/tokenize.rs` informs search prep workers, not request handlers.
- `spank-rs/perf/src/store.rs` informs null/raw/SQLite benchmark sinks, not product storage schema.
- `spank-hec/src/receiver.rs` informs queue/backpressure ordering, but its Axum `Bytes` extractor path should not be lifted over current bounded body read.
- `spank-hec/src/processor.rs` informs event/null/time/parser tests, but its raw and gzip limits are weaker than current code.

The immediate reusable architecture is therefore:

```text
HEC parser -> bounded EventBatch -> queue -> sink worker -> capture/hot bucket -> replay/search-prep workers
```

That is the bridge from current correctness-focused receiver to later high-throughput parsing and indexing.
