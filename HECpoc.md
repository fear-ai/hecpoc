# HECpoc — Focused HEC Proof Of Concept Starting Point

HECpoc is a fresh Rust implementation effort for a small, testable HTTP Event Collector receiver. The first product is a local endpoint that accepts realistic HEC traffic, preserves events, exposes enough inspection to assert what arrived, and makes compatibility differences explicit.

The starting user is a developer or CI engineer who wants to test code that sends logs to Splunk HEC without running full Splunk for every test. The immediate benefit is practical: catch bad tokens, malformed payloads, missing metadata, gzip mistakes, raw endpoint surprises, retry behavior, and storage/inspection mismatches before production.

The document starts with implementation guardrails, then moves through scope, protocol, sinks, validation, Rust structure, mainstay infrastructure, open work, and references.

---

## 1. Implementation Guardrails

These guardrails constrain the first implementation. They are here to reduce ambiguity, dependency creep, and inherited architecture debt.

### 1.1 Scope

The initial scope is HEC ingest, local capture, inspection, and validation. Search, parser specialization, Sigma, retention, repair, TLS hardening, full ACK semantics, and performance-specific storage enter only after the HEC path proves correct enough to need them.

Prior attempts are not part of the active design. They are cataloged in `History.md` only as evidence for narrow questions.

### 1.2 Naming and layout

Names should match the domain and data direction. Prefer `Event`, `HecEvent`, `Collector`, `EventSink`, `CaptureSink`, `SinkCommit`, and `InspectQuery`. Avoid `Row` and `Sender` in ingest code unless the role is truly generic and direction-free.

Rename directories, files, and implementation primitives when the current names obscure function, compatibility, or review. Regularity is a feature here because the repo must be understandable by a small team.

### 1.3 Dependencies and errors

Minimize dependencies. Add crates only when need and behavior are clear.

Initial posture:

- HTTP stack: use Tokio plus Axum, with Axum kept at the adapter boundary.
- Metrics: simple counters or structured run output first; Prometheus later if metrics become an external interface.
- Middleware: avoid Tower-style hidden layers until repeated cross-cutting behavior exists.
- Locks: standard locking first; the PoC is not initially a high-concurrency design exercise.
- Storage: generic event sink boundary, concrete file capture first; no early SQLite tuning.
- Errors: keep HEC protocol outcomes distinct from internal implementation errors at call sites.
- Runtime: use Tokio; prefer Axum as a thin HTTP adapter over a framework-free collector core, with Hyper direct remaining the escape hatch if exact protocol control requires it.

### 1.4 Performance and resilience posture

Performance matters from the start, but the first goal is measured simplicity rather than clever machinery. The PoC should make CPU, memory, concurrency, and failure behavior visible before optimizing them.

Initial posture:

- CPU: parse in straightforward code first; keep CPU-heavy parsing, tokenization, indexing, compression expansion, and future regex work out of ordinary request-handler critical sections when they become measurable.
- Memory: enforce request body limits, decoded body limits, line limits, and bounded queues; avoid unbounded buffering per connection or per request.
- Concurrency: use Tokio for network concurrency, bounded channels for handoff, and explicit worker boundaries for sink writes; avoid shared mutable state unless ownership through message passing is worse.
- Backpressure: reject overload at admission or queue handoff with an explicit retryable response rather than awaiting indefinitely.
- Resilience: define accepted, queued, captured, flushed, and durable as separate states; do not let a success response imply a stronger state than the implementation actually achieved.
- Degradation: when sink failures or queue saturation persist, expose that through health/run output before adding a full phase machine.

### 1.5 Tests and documentation

Unit tests live with the unit or crate they test. Integration and system tests live where they exercise real process, socket, file, and tool boundaries.

Code comments should explain local invariants, protocol quirks, and non-obvious decisions. Issue IDs, task history, and intermediate status belong in workbench documents and run ledgers, not product code.

---

## 2. Scope, Wants, and Capability Bundles

The scope starts from user wants, then derives features. It should not be organized around every Splunk feature that can be named.

### 2.1 Wants, features, benefits

The user wants to start a local endpoint, send events using ordinary HEC clients, see clear success or failure, inspect accepted events, compare selected behavior with Splunk, and repeat the same run in development and CI.

| Feature | Benefit |
|---------|---------|
| HEC JSON ingest | Applications and shippers use their real output path |
| Raw ingest | Raw endpoint users and line senders can be tested |
| Token auth | Bad-token and missing-auth failures are caught |
| Gzip decode | Common compressed client behavior is covered |
| Metadata capture | Tests assert time, host, source, sourcetype, and index |
| File capture sink | Accepted events are directly inspectable |
| Backpressure response | Overload becomes visible, not silently accepted |
| Local inspection | Tests assert stored output without reading internals |
| Bounded resource use | Bad inputs and slow sinks do not consume unbounded memory or worker time |
| Resilient failure reporting | Users can distinguish rejected, accepted, captured, and failed-after-accept cases |

### 2.2 Capability bundles

Group capabilities by functional bundle and likely sequence, not by requirement prefix.

| Bundle | Contents | Stage |
|--------|----------|-------|
| A. JSON, raw, files | `ING-HEC-JSON`, `ING-HEC-RAW`, `EVT-RAW`; visible file/capture evidence | First |
| B. Backpressure | `ING-BACKPRESS`; explicit retryable failure under saturation | First |
| C. Time and metadata | `EVT-TIME`, `EVT-HOST`, `EVT-SOURCE`, `EVT-SOURCETYPE`; event identity | First |
| D. Auth and gzip | `ING-HEC-AUTH`, `ING-HEC-GZIP`; realistic client behavior | First |
| E. Inspection | `SCH-TERM`, `SCH-TIME`, maybe `SCH-FIELDS`; assertion surface | Early |
| F. More sinks, index, metrics | `EVT-INDEX`, `OBS-METRICS`, durable sink work | Later |
| G. ACK and capability metadata | `ING-HEC-ACK`, `PAR-CAP`; commit and parser capability semantics | Later |
| H. Resource and resilience controls | body limits, queue limits, slow-sink behavior, health degradation | First |

### 2.3 Initial detail level

Capture concrete requirements, high-level architecture, event/sink/validation design, and only the low-level details that block implementation. Work decomposition should stay short. Validation is designed alongside code, not appended after it.

---

## 3. Protocol and Event Semantics

Protocol design is the first technical center of gravity. It defines which entities exist, how requests move through states, and what behavior must be tested.

### 3.1 Request phases and entities

Request states:

```text
HTTP request
  -> admitted or rejected
  -> authenticated or rejected
  -> decoded or rejected
  -> parsed as event/raw or rejected
  -> normalized into events
  -> submitted to sink or rejected by backpressure
  -> captured or committed
  -> inspectable
```

Core entities:

| Entity | Meaning |
|--------|---------|
| `HecRequest` | Method, path, headers, body, peer context |
| `HecCredential` | Parsed auth scheme and token |
| `HecEnvelope` | JSON event endpoint envelope |
| `HecEvent` | Normalized accepted event |
| `EventBatch` | One request's accepted events |
| `SinkCommit` | Sink result visible to validation |
| `InspectQuery` | Minimal read path over captured events |

Each transition should have a validation case before surrounding code is considered stable.

### 3.2 Endpoint behavior

Minimum surface:

- `/services/collector/event`: accept one or more JSON envelopes.
- `/services/collector/raw`: accept newline-framed raw events with documented CRLF behavior.
- `/services/collector/health`: report simple availability.
- `/services/collector/ack`: return a deliberate disabled/unsupported response until commit semantics exist.
- Body encoding: identity and gzip, with explicit pre-decode and post-decode size policy.
- Admission controls: bounded request body, decoded body, per-event raw size, and bounded sink handoff.

Route aliases such as `/services/collector/1.0/*` should wait for client evidence.

### 3.3 Event fields

Initial field rules:

- `_raw`: preserve event text for comparison; raw byte preservation can become a later sink property.
- `_time`: store parsed event time with an explicit precision decision. Do not assume nanoseconds before Splunk comparison and storage design; microseconds may be enough, nanoseconds may be convenient internally.
- `host`, `source`, `sourcetype`: store payload values and make defaults visible.
- `index`: logical namespace first; physical partitioning later.
- `fields`: start with flat scalar values; nested behavior must be accepted, ignored, or rejected deliberately.

### 3.4 Invalid and questionable input

The main design must capture corner cases because they shape parser, error, sink, and validation code.

| Group | Cases |
|-------|-------|
| Auth | missing, malformed, wrong scheme, empty token, invalid token, valid token |
| JSON | empty, malformed, multiple envelopes, later invalid envelope, event absent, null, empty string, object, array |
| Raw | empty body, trailing newline, CRLF, blank line, whitespace-only line, invalid UTF-8 if text output is used |
| Gzip and size | valid gzip, malformed gzip, empty decoded body, pre-decode limit, post-decode limit |
| Metadata | missing values, explicit empty strings, nested fields, non-scalar fields |
| Backpressure | full queue, slow sink, sink error after accepted handoff |
| ACK/channel | channel absent, channel empty, channel present with ACK disabled, ACK request before implementation |

---

## 4. Sink, Store, and Inspection Strategy

Sink choice is part of ingest correctness. The first implementation should prove accepted events are visible before it designs a database.

### 4.1 Sink order

Sort by usefulness and complexity:

1. Capture file sink: first correctness evidence.
2. In-memory assertion sink: useful once tests need direct event access.
3. Null sink: benchmark only, not correctness.
4. Raw chunk or structured file sink: later replay and corruption checks.
5. SQLite or queryable store: later durable local query; no early optimization.
6. External forwarding sink: defer; that is another product mode.

The first practical path is capture file plus simple inspection.

### 4.2 Inspection path

Start close to stored evidence: write accepted events to a documented file format, provide a tiny inspection command or test helper, support term/time filters only after semantics are defined, and add indexing only when the simple path fails.

A sink trait is justified only when two concrete implementations need the same call sites and can be tested independently. Until then, a concrete capture sink is simpler than an abstraction display case.

### 4.3 Resource and failure boundaries

The sink boundary is where concurrency and resilience become concrete. The collector should not let request handlers become file-system workers under load.

Initial rules:

- Request handlers may parse and normalize small HEC bodies, but they should not perform long blocking writes.
- Sink writes should happen through a bounded handoff or a clearly synchronous fixture mode; the chosen mode must be visible in validation.
- Queue depth, max request bytes, max decoded bytes, max raw event bytes, and max events per request should be configurable or at least named constants.
- Slow sink behavior should be tested by a deliberately blocking or failing sink.
- Capture files should use buffered writes, but flush semantics must be tied to explicit validation expectations.
- Crash resilience is limited at first: file capture should be append-only and inspectable after process exit, but not advertised as durable ACK storage.

---

## 5. Validation Strategy

Validation starts from wants and needs, then checks compatibility and capability. Tests fit under that structure.

| Level | Question | Evidence |
|-------|----------|----------|
| Wants and needs | Does this catch real HEC integration mistakes? | local run with inspectable output |
| Compatibility | Does selected behavior match Splunk or known clients? | curl/Vector/Splunk comparison |
| Protocol | Are phases and edge cases deliberate? | unit and handler tests |
| Sink | Do accepted events match stored evidence? | capture inspection |
| Resource use | Are CPU, memory, and queue limits enforced? | limit tests and run counters |
| Resilience | Are rejected, accepted, captured, and failed states distinguishable? | slow/failing sink tests |
| Usability | Can a developer run it without reading source? | README command sequence |
| Prioritization | Does each feature activate the first workflow? | capability bundle table |

Validation layers are unit tests, handler tests, process tests with curl, Vector shipper tests, Splunk Enterprise comparison, tutorial-log corpus runs, and later benchmarks.

Performance validation starts modestly: report request count, accepted event count, rejected count, bytes in, decoded bytes, max-body rejection, queue-full count, sink write failures, and elapsed time. CPU and RSS measurements can be coarse at first, but each benchmark run should record enough machine and config context to avoid comparing ghosts.

Artifacts stay small:

```text
fixtures/
  requests/        curl bodies and expected response snippets
  scripts/         thin wrappers for curl, Vector, and inspection
  configs/         small Spank, Vector, and Splunk notes/config fragments
  logs/            tiny copied fixtures only
results/
  README.md        ledger schema and dated run summaries
```

Large logs remain under `/Users/walter/Work/Spank/Logs`.

---

## 6. Rust Implementation Shape

Rust structure should follow capability and protocol boundaries. Avoid fine-chopping crates before internal completeness, consistency, external reuse, or planned mix-and-match justify the split.

Use Tokio for the server runtime. The default starting shape is Axum as a thin adapter over framework-free collector functions. Hyper direct remains a later option if Axum makes body control, graceful shutdown, or protocol conformance harder to prove.

### 6.1 Initial layout

Start as one small crate with `hec_receiver/` under `src/`:

```text
src/
  main.rs
  config.rs
  error.rs
  hec_receiver/
    mod.rs
    protocol.rs      endpoint paths, request phases, response mapping
    body.rs          size policy and gzip/identity decode
    auth.rs          auth header parsing and token validation
    event.rs         JSON envelope and HEC event normalization
    raw.rs           raw endpoint framing and newline policy
    sink.rs          capture sink and minimal sink boundary
    inspect.rs       simple readback over capture files
    limits.rs        resource limits and admission constants/config

tests/
  hec_protocol.rs
  hec_process.rs

fixtures/
  requests/
  scripts/
  configs/
  logs/
```

Avoid redundant `spank-` prefixes inside this repo.

### 6.2 Crate and trait guidance

A crate split is justified only when the candidate crate is internally complete and separately useful: reusable collector, protocol parser, store, or benchmark harness. Until then, modules are cheaper than crates.

A trait is justified only after separate implementations prove a real boundary. Limit generic boilerplate.

### 6.3 Mainstay Infrastructure

The implementation should converge on stable infrastructure rather than local one-off mechanisms.

Configuration:

- `clap` for CLI parsing, help, version output, and completions;
- `serde` typed structs for the configuration model;
- `figment` for provider layering: defaults, TOML, CLI, environment;
- TOML as the persistent operator-facing file format;
- frozen startup configuration for the foreseeable future;
- validation after merge and before bind, using explicit validation methods or `validator` where it remains readable.

Errors and outcomes:

- HEC protocol outcomes are centralized definitions of text, result code, HTTP status, and public response body;
- internal errors are typed enough to classify configuration, startup, auth, body, decode, parse, queue, sink, and shutdown failures;
- handlers should call named constructors rather than hand-writing strings or status mappings;
- sink results explicitly distinguish accepted, queued, captured, flushed, and durable states.

Logging and status:

- startup logs include effective redacted configuration, listener address, runtime shape, selected sink, selected parser/splitter, and binary/build metadata;
- request logs and counters use canonical names plus original evidence fields where needed;
- shutdown logs include reason, uptime, and final counters;
- notification is initially process exit status, structured logs, stats endpoint, and benchmark ledger output.

Timing and performance:

- record timings by stage rather than one aggregate request duration;
- benchmark ledgers capture config, corpus, tool, version, host, CPU, OS, bytes/sec, events/sec, errors, and latency where available;
- optimization work must name the stage it improves: body read, gzip decode, framing, JSON parse, enqueue, sink write, flush, or durable commit.

Dependencies:

- mainstay early crates are `tokio`, `axum`, `serde`, `serde_json`, `toml`, `figment`, `clap`, gzip support, and test support;
- metrics, Prometheus, direct Hyper, database crates, and optimized parsers are added only when the active capability bundle needs them;
- a crate split is justified only when a component is internally complete and separately useful.

For the HTTP stack, start with `tokio` plus `axum`. Axum terminates at an adapter layer that converts HTTP requests into HECpoc request objects and HECpoc outcomes back into HTTP responses. The protocol, event, sink, configuration, and validation logic should not depend on Axum extractors.

Concurrency model:

- one Tokio runtime for network and coordination;
- bounded request-to-sink handoff for normal mode;
- optional direct synchronous sink only for tiny fixture mode;
- no blocking filesystem calls in async handlers unless isolated and measured;
- CPU-heavy parser/index work moves behind explicit worker boundaries, not casual `tokio::spawn`.

---

## 7. Reference Boundaries

The controlling design documents for this repo are this file, `Config.md`, and `Stack.md`. They describe what we are building. Prior attempts and older code are not implementation plans.

Prior material may still be useful as evidence when it answers a specific question, but it must not drive naming, layout, configuration, error handling, concurrency, sink semantics, or validation behavior. Historical notes live in `History.md` and are non-authoritative.

Code or ideas from older repos can enter HECpoc only through a current design decision:

1. name the requirement or capability it satisfies;
2. restate it in HECpoc vocabulary;
3. add tests for valid, invalid, edge, overload, and hostile cases;
4. verify dependencies and runtime assumptions;
5. record benchmark evidence if performance is the reason;
6. update current docs instead of reviving old status notes.

---

## 8. Initial Work Sequence

The first sequence should build mainstay infrastructure while preserving protocol evidence.

1. Replace configuration loading with `clap` + `figment`, implement `--config`, `--show-config`, and `--check-config`.
2. Centralize protocol outcome text, result codes, HTTP status mappings, config field names, policy names, and metric labels.
3. Add structured startup errors and request-path error classification for auth, body, gzip, parse, queue, and sink failures.
4. Add explicit `LineSplitter` behavior and tests for LF, CRLF, NUL, control bytes, non-ASCII, invalid UTF-8, long lines, and no-final-newline.
5. Create request fixtures and process tests for JSON event, raw, gzip, malformed auth, malformed JSON, bad gzip, oversize bodies, and no-data bodies.
6. Define capture sink states and then insert bounded enqueue/dequeue between request handling and sink writing.
7. Add benchmark ledger output for single-stream and concurrent curl/`oha`/`wrk` runs with bytes/sec and events/sec.
8. Compare selected cases with local Splunk Enterprise and Vector after the local behavior is stable enough to reproduce.

First target:

```text
merge config -> validate -> bind -> accept HEC JSON/raw -> classify errors -> capture event -> inspect capture -> record run evidence
```


## 9. Open Work, Gaps, and Decisions

This register keeps the unresolved work visible without forcing everything into one artificial component or decision ID chain. Items here are not a replacement for tests or implementation; they are the current map of what must be settled to make the PoC coherent.

### 9.1 Product and scope

| Area | Gap or question | Near-term action |
|------|-----------------|------------------|
| Primary user | The first user is a CI/developer user, but exact packaging is not settled | Decide whether the first UX is binary plus shell tests, Rust test helper, or later pytest wrapper |
| Fixture versus emulator | HEC fixture scope can expand into exact HEC emulator behavior | Keep first bundle fixture-oriented; classify emulator-only features separately |
| Compatibility language | "Splunk-compatible" is too broad | Use capability-specific language: HEC JSON-compatible, Vector-sendable, Splunk-compared |
| Requirement subset | HECpoc still needs its own filtered matrix | Create local HEC-only requirement table from `Features.csv` |
| Bundling | Capability bundles A-G are draft groupings | Validate by mapping each to one runnable user workflow |

### 9.2 Protocol behavior

| Area | Gap or question | Near-term action |
|------|-----------------|------------------|
| Endpoint aliases | `/services/collector/1.0/*` and SDK legacy paths are undecided | Test curl, Vector, and any local SDK client before adding aliases |
| Auth errors | Missing, malformed, bad scheme, empty token, and invalid token may need distinct outcomes | Compare Splunk Enterprise and choose stable response mapping |
| `event` semantics | Absent, null, empty string, object, array, number, and boolean need explicit behavior | Build a protocol matrix and tests before parser reuse |
| `fields` semantics | Flat scalar, nested object, array, and non-string handling are unsettled | Start flat/scalar; classify nested behavior as reject, stringify, or ignore |
| Time precision | Internal precision is not yet decided | Compare Splunk output, JSON number precision, and sink format; choose microseconds or nanoseconds explicitly |
| Raw endpoint | CRLF, blank lines, whitespace-only lines, invalid UTF-8, and metadata query params are unsettled | Define raw framing before implementing raw as more than a smoke path |
| Gzip limits | Pre-decode and post-decode size behavior both matter | Enforce and test both or explicitly defer decoded-size cap |
| ACK disabled | The response for `/ack` before ACK support is unsettled | Compare Splunk with ACK disabled and Vector behavior |
| Channel handling | Header, empty header, query channel, UUID validation, and ACK interaction are unsettled | Implement non-ACK channel only after response behavior is documented |

### 9.3 Event, sink, and inspection

| Area | Gap or question | Near-term action |
|------|-----------------|------------------|
| Capture format | The first file format is not selected | Choose JSONL or length-delimited JSON and document exact fields |
| Raw preservation | Text preservation and byte preservation are not the same | Start with text comparison; plan raw-byte preservation before replay claims |
| Sink commit | Accepted, queued, captured, flushed, and durable are different states | Name these states and decide what HTTP success means |
| Sink failure | A sink may fail after request acceptance | Decide whether failure changes health, phase, metrics, or only run ledger |
| Inspection API | Term/time inspection is needed, but exact command/interface is unsettled | Start with one readback helper over capture files |
| Index namespace | `index` is logical first, physical later | Store as event field; defer partitioning |
| Metrics | Counters are useful, but Prometheus is likely premature | Emit simple counters/run summaries first |
| CPU budget | Parser, gzip, and sink work may dominate before HTTP does | Measure parse/decode/write time separately before optimizing framework choice |
| Memory budget | Large request bodies, gzip expansion, and per-request event vectors can grow quickly | Define max body, decoded body, event count, and raw event size limits |
| Concurrency | Slow clients and slow sinks can consume runtime capacity differently | Separate network concurrency from sink write concurrency with bounded handoff |
| Resilience state | Success response, file append, flush, fsync, and ACK durability are distinct | Name the strongest state actually reached by each mode |

### 9.4 Validation and fixtures

| Area | Gap or question | Near-term action |
|------|-----------------|------------------|
| Golden request set | Positive and negative request bodies need to exist as files | Create `fixtures/requests/` with JSON, raw, gzip, and malformed cases |
| Splunk comparison | Vendor docs are insufficient for edges | Run selected cases against local Splunk Enterprise and record differences |
| Vector validation | Vector may exercise batching, retries, compression, and channel behavior differently than curl | Add Vector only after curl/capture path works |
| Corpus scope | Tutorial and production logs are large and varied | Keep tiny fixtures local; reference large corpora by path |
| Ledger format | Results need enough context to reproduce | Record command, config, git revision, input, expected count, accepted count, response codes, sink path |
| Test layering | Unit, integration, and process tests need clear homes | Keep unit tests near modules; use `tests/` for process/socket/file validation |

### 9.5 Rust implementation

| Area | Gap or question | Near-term action |
|------|-----------------|------------------|
| HTTP stack | Tokio plus Axum is the selected starting stack | Keep Axum at the adapter boundary and preserve direct request/body visibility |
| Error structure | Central definitions are required for protocol outcomes, internal errors, sink states, and config errors | Implement named constructors and avoid scattered message/status literals |
| Traits | Generic interfaces can proliferate too early | Add traits only after two implementations prove the boundary |
| Crate split | Multiple crates are not justified yet | Start one crate with `src/hec_receiver/` |
| Locking | Concurrency needs are modest initially | Use standard locks or no locks until measured need appears |
| Dependency audit | Dependency additions must match active capability bundles | Add crates through the current design, not inherited lists |
| Naming | Ambiguous names hide data direction and state | Prefer event/source/context/sink/commit vocabulary |
| CPU work in async | Heavy parser/index work can starve Tokio workers | Keep initial parse small; move heavy work to explicit workers when measured |
| HTTP framework | Axum is likely sufficient but can hide body/extractor behavior | Use Axum as adapter; keep Hyper direct as escape hatch |

---

## 10. References

References are split into current controlling documents and external comparison points. Historical project material is cataloged separately in `History.md` and is not authoritative for this repo.

### 10.1 Current project documents

1. `/Users/walter/Work/Spank/HECpoc/Config.md` — configuration surface, merge order, validation, status logging, timing, benchmark recording, and centralized definitions.
2. `/Users/walter/Work/Spank/HECpoc/Stack.md` — HTTP/Tokio/Axum stack, byte processing stages, raw splitting, backpressure, buffering, security, and performance considerations.
3. `/Users/walter/Work/Spank/HECpoc/PerfIntake.md` — performance-oriented intake notes that should influence benchmark design without becoming product architecture.
4. `/Users/walter/Work/Spank/HECpoc/History.md` — non-authoritative catalog of prior attempts and abandoned approaches.

### 10.2 External comparison points

1. [Splunk: Format events for HTTP Event Collector](https://docs.splunk.com/Documentation/Splunk/latest/Data/FormateventsforHTTPEventCollector) — JSON envelope and metadata examples.
2. [Splunk: Troubleshoot HTTP Event Collector](https://docs.splunk.com/Documentation/Splunk/latest/Data/TroubleshootHTTPEventCollector) — error/status behavior.
3. [Vector `splunk_hec_logs` sink](https://vector.dev/docs/reference/configuration/sinks/splunk_hec_logs/) — real HEC client behavior, batching, ACK, retry, TLS.
4. [Fluent Bit Splunk output](https://docs.fluentbit.io/manual/data-pipeline/outputs/splunk) — common shipper configuration vocabulary.
5. OpenTelemetry Collector contrib `splunkhecreceiver` — server-side implementation reference.
6. Local Splunk Enterprise — ground truth for selected edge cases when docs and clients disagree.
