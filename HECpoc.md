# HECpoc — Focused HEC Proof Of Concept Starting Point

HECpoc is a fresh Rust implementation effort for a small, testable HTTP Event Collector receiver. The first product is a local endpoint that accepts realistic HEC traffic, preserves events, exposes enough inspection to assert what arrived, and makes compatibility differences explicit.

The starting user is a developer or CI engineer who wants to test code that sends logs to Splunk HEC without running full Splunk for every test. The immediate benefit is practical: catch bad tokens, malformed payloads, missing metadata, gzip mistakes, raw endpoint surprises, retry behavior, and storage/inspection mismatches before production.

The document defines the product slice, HEC protocol behavior, event/sink semantics, and open product decisions. Cross-cutting infrastructure is concentrated in `InfraHEC.md`; network/Tokio/HTTP mechanics are concentrated in `Stack.md`.

---

## 1. Product Guardrails

### 1.1 Scope

The initial scope is HEC ingest, local capture, inspection, and validation. Search, parser specialization, Sigma, retention, repair, TLS hardening, full ACK semantics, and performance-specific storage enter only after the HEC path proves correct enough to need them.

Prior attempts are not part of the active design. They are cataloged in `docs/History.md` only as evidence for narrow questions.

### 1.2 Product Vocabulary

Names should match the domain and data direction. Prefer `Event`, `HecEvent`, `Collector`, `EventSink`, `CaptureSink`, `SinkCommit`, and `InspectQuery`. Avoid `Row` and `Sender` in ingest code unless the role is truly generic and direction-free.

Rename directories, files, and implementation primitives when the current names obscure function, compatibility, or review. Regularity is a feature here because the repo must be understandable by a small team.


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

## 5. Reference Boundaries

This file defines the HEC product slice and protocol-facing decisions. `InfraHEC.md` defines cross-cutting implementation infrastructure. `Stack.md` records detailed network ingress, Tokio/Axum/Hyper, buffering, body-processing, and kernel/runtime mechanics.

Prior material may still be useful as evidence when it answers a specific question, but it must not drive naming, layout, configuration, error handling, concurrency, sink semantics, or validation behavior. Historical notes live in `docs/History.md` and are non-authoritative.

Code or ideas from older repos can enter HECpoc only through a current design decision:

1. name the requirement or capability it satisfies;
2. restate it in HECpoc vocabulary;
3. add tests for valid, invalid, edge, overload, and hostile cases;
4. verify dependencies and runtime assumptions;
5. record benchmark evidence if performance is the reason;
6. update current docs instead of reviving old status notes.

---

## 6. Initial Work Sequence

The first sequence keeps product behavior and implementation infrastructure aligned without duplicating the infrastructure spec here.

1. Implement configuration and startup infrastructure from `InfraHEC.md §§7, 11, 20`.
2. Centralize HEC outcomes and request error mapping from `InfraHEC.md §§8, 10, 20`.
3. Lock raw/event protocol behavior for auth, body, gzip, JSON envelopes, raw line framing, and no-data cases.
4. Create request fixtures for JSON event, raw, gzip, malformed auth, malformed JSON, bad gzip, oversize bodies, and no-data bodies.
5. Implement capture sink states enough to distinguish accepted, queued, captured, flushed, and durable claims.
6. Add bounded handoff between request processing and sink writing once direct capture behavior is stable.
7. Run local curl/process tests, then selected Splunk Enterprise and Vector comparisons.
8. Record benchmark and validation evidence using `InfraHEC.md §§17–18`.

First target:

```text
merge config -> validate -> bind -> accept HEC JSON/raw -> classify errors -> capture event -> inspect capture -> record run evidence
```


## 7. Open Product Work, Gaps, and Decisions

This register keeps the unresolved work visible without forcing everything into one artificial component or decision ID chain. Items here are not a replacement for tests or implementation; they are the current map of what must be settled to make the PoC coherent.

### 7.1 Product And Scope

| Area | Gap or question | Near-term action |
|------|-----------------|------------------|
| Primary user | The first user is a CI/developer user, but exact packaging is not settled | Decide whether the first UX is binary plus shell tests, Rust test helper, or later pytest wrapper |
| Fixture versus emulator | HEC fixture scope can expand into exact HEC emulator behavior | Keep first bundle fixture-oriented; classify emulator-only features separately |
| Compatibility language | "Splunk-compatible" is too broad | Use capability-specific language: HEC JSON-compatible, Vector-sendable, Splunk-compared |
| Requirement subset | HECpoc still needs its own filtered matrix | Create local HEC-only requirement table from `Features.csv` |
| Bundling | Capability bundles A-G are draft groupings | Validate by mapping each to one runnable user workflow |

### 7.2 Protocol Behavior

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

### 7.3 Event, Sink, And Inspection

| Area | Gap or question | Near-term action |
|------|-----------------|------------------|
| Capture format | The first file format is not selected | Choose JSONL or length-delimited JSON and document exact fields |
| Raw preservation | Text preservation and byte preservation are not the same | Start with text comparison; plan raw-byte preservation before replay claims |
| Sink commit | Accepted, queued, captured, flushed, and durable are different states | Name these states and decide what HTTP success means |
| Sink failure | A sink may fail after request acceptance | Decide whether failure changes health, phase, metrics, or only run ledger |
| Inspection API | Term/time inspection is needed, but exact command/interface is unsettled | Start with one readback helper over capture files |
| Index namespace | `index` is logical first, physical later | Store as event field; defer partitioning |
| Resilience state | Success response, file append, flush, fsync, and ACK durability are distinct | Name the strongest state actually reached by each mode |

---

## 8. References

References are split into current project documents and external comparison points. Historical project material is cataloged separately in `docs/History.md` and is not authoritative for this repo.

### 8.1 Current Project Documents

1. `/Users/walter/Work/Spank/HECpoc/InfraHEC.md` — infrastructure implementation spine; specific current anchors are §7 config, §8 errors/outcomes/reporting, §11 lifecycle, §17 validation, §18 benchmarks, and §20 sequence.
2. `/Users/walter/Work/Spank/HECpoc/Stack.md` — network ingress and request-processing ledger; specific current anchors are §§6–12 Tokio/Axum/auth/gzip/body/timeouts, §28 accept/receive paths, §30 kernel/runtime knobs, §§35–36 buffering and raw splitting.
3. `/Users/walter/Work/Spank/HECpoc/docs/PerfIntake.md` — performance intake notes for benchmark design and optimization admission.
4. `/Users/walter/Work/Spank/HECpoc/docs/History.md` — non-authoritative catalog of prior attempts and abandoned approaches.

### 8.2 External Comparison Points

1. [Splunk: Format events for HTTP Event Collector](https://docs.splunk.com/Documentation/Splunk/latest/Data/FormateventsforHTTPEventCollector) — JSON envelope and metadata examples.
2. [Splunk: Troubleshoot HTTP Event Collector](https://docs.splunk.com/Documentation/Splunk/latest/Data/TroubleshootHTTPEventCollector) — error/status behavior.
3. [Vector `splunk_hec_logs` sink](https://vector.dev/docs/reference/configuration/sinks/splunk_hec_logs/) — real HEC client behavior, batching, ACK, retry, TLS.
4. [Fluent Bit Splunk output](https://docs.fluentbit.io/manual/data-pipeline/outputs/splunk) — common shipper configuration vocabulary.
5. OpenTelemetry Collector contrib `splunkhecreceiver` — server-side implementation reference.
6. Local Splunk Enterprise — ground truth for selected edge cases when docs and clients disagree.
