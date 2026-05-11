# HECpoc — Focused HEC Proof Of Concept Starting Point

HECpoc is a fresh Rust implementation effort for a small, testable HTTP Event Collector receiver. The first product is a local endpoint that accepts realistic HEC traffic, preserves events, exposes enough inspection to assert what arrived, and makes compatibility differences explicit.

The starting user is a developer or CI engineer who wants to test code that sends logs to Splunk HEC without running full Splunk for every test. The immediate benefit is practical: catch bad tokens, malformed payloads, missing metadata, gzip mistakes, raw endpoint surprises, retry behavior, and storage/inspection mismatches before production.

This document defines the product contract, protocol behavior, capability bundles, staged decisions, documentation map, and inclusion rules for the HECpoc documentation set.

---

## 1. Product Guardrails

### 1.1 Scope

The initial scope is HEC ingest, local capture, inspection, and validation. Search, parser specialization, Sigma, retention, repair, TLS hardening, full ACK semantics, and performance-specific storage enter only after the HEC path proves correct enough to need them.

### 1.2 Product Vocabulary

Names should match the domain and data direction. The selected terminology is governed by Appendix 11: `HecEnvelope`, `RequestRaw`, `RawEvents`, `HecEvents`, `ParseBatch`, `WriteBlock`, concrete dispositions, and truthful commit states. Avoid `Row`, `Sender`, generic `worker`, and generic `Batch` unless the role is truly generic and direction-free.

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
| Resilient failure reporting | Users can distinguish rejected, accepted, written, flushed, durable, and failed-after-accept cases |

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
  -> authenticated or rejected
  -> HTTP body read or rejected
  -> content decoded or rejected
  -> HEC decoded or rejected
  -> HEC events validated or rejected
  -> HecEvents formed
  -> concrete disposition selected
  -> commit state recorded
  -> inspectable when stored
```

Core entities:

| Entity | Meaning |
|--------|---------|
| `HecRequest` | Method, path, headers, body, peer context |
| `HecCredential` | Parsed auth scheme and token |
| `HecEnvelope` | JSON event endpoint envelope |
| `HecEvent` | Normalized accepted event |
| `HecEvents` | Valid HEC events from one request after HEC decode and validation |
| commit state | Strongest completed state visible to response, ACK, validation, and reporting |
| `InspectQuery` | Minimal read path over stored capture evidence |

Each transition should have a validation case before surrounding code is considered stable.

### 3.2 Endpoint behavior

Minimum surface:

- `/services/collector/event`: accept one or more JSON envelopes.
- `/services/collector/raw`: accept newline-framed raw events with documented CRLF behavior.
- `/services/collector/health`: report simple availability.
- `/services/collector/ack`: return a deliberate disabled/unsupported response until commit semantics exist.
- Body encoding: identity and gzip, with explicit pre-decode and post-decode size policy.
- Resource gates: bounded HTTP body, decoded body, per-event raw size, event count, and bounded queue insertion when queue mode exists.

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
| Backpressure | full queue, slow sink, sink error after accepted write/queue disposition |
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

The core design choice is whether validated `HecEvents` are written synchronously in fixture mode or inserted into a bounded queue for later `WriteBlock` construction. The collector should not let request handlers perform long file or database work under load.

Initial rules:

- Request handlers may HEC-decode and validate small bounded bodies, but they should not perform long blocking writes.
- The chosen disposition must be visible in validation: `write HecEvents`, `enqueue HecEvents`, `drop HecEvents`, `forward HecEvents`, or `reject request`.
- Queue depth, max request bytes, max decoded bytes, max raw event bytes, and max events per request should be configurable or at least named constants.
- Slow write behavior should be tested by a deliberately blocking or failing sink/store path.
- Capture files should use buffered writes, but flush semantics must be tied to explicit validation expectations.
- Crash resilience is limited at first: file capture should be append-only and inspectable after process exit, but not advertised as durable ACK storage.

---

## 5. Documentation Architecture And Inclusion Rules

This section is the HECpoc documentation map. Subject-specific documents should not repeat this map or carry generic file-purpose lists. Each file states only its own scope and the technical subject it owns.

| File | Focus | Includes | Excludes |
|---|---|---|---|
| `HECpoc.md` | product and protocol control plane | user goals, capability bundles, HEC request/event contract, staged decisions, acceptance gates, documentation map | deep parser grammars, OS/socket mechanics, implementation infrastructure internals |
| `InfraHEC.md` | cross-cutting service infrastructure | configuration, validation, errors/outcomes, reporting/logging/observability, metrics, lifecycle, runtime policy, security posture, benchmark ledger schema | log-format grammars, queue/store algorithms, socket syscall details |
| `Stack.md` | ingress and operating-system stack | TCP/HTTP/Tokio/Axum/Hyper, auth/body/gzip/timeouts, kernel socket buffers, page cache notes, system calls, connection accounting, network-layer backpressure | log-line grammars, token/index layout, store retirement policy |
| `Formats.md` | log and record structure | source format origins, examples, version splits, parser choices, field extraction, field aliases, malformed record cases, format-specific parser validation | generic OS buffering, HEC status mapping, queue topology, durable store layout |
| `Store.md` | application pipeline and stored evidence | `HecEvents` disposition, queue topology, `ParseBatch` policy, `WriteBlock` construction, commit states, durable commit, intermediate store, token/index construction, production/benchmark profile differences | kernel/socket mechanics, detailed log-format syntax, generic reporting infrastructure |

Inclusion rules:

1. A topic belongs where its primary design variable lives, not where it was first discussed.
2. Validation belongs with the subsystem whose behavior is being proven. Protocol response validation belongs here; socket/header timeout validation belongs in `Stack.md`; parser correctness validation belongs in `Formats.md`; queue/store/durability validation belongs in `Store.md`; report/config validation belongs in `InfraHEC.md`.
3. References should be specific evidence for the local subject. Avoid empty mentions of another project document just to say it exists.
4. Stable requirements and justified recommendations stay in reference sections. Work tracking and status tables are kept short and only when they control the next implementation step.
5. External or historical code can influence HECpoc only after restating the current requirement, naming the implementation target, adding validation cases, and recording why the approach remains suitable.

Decision validity classes:

| Class | Meaning | Example | Revisit Trigger |
|---|---|---|---|
| Contract | external HEC behavior expected to remain stable | accepted endpoints, auth response shape, event metadata preservation | Splunk/shipper comparison contradicts it |
| Capability bundle | valid for a named feature group | local fixture capture sink, compatibility lab behavior, durable ingest mode | bundle scope changes or user workflow changes |
| Implementation stage | valid for current implementation stage | direct capture before bounded queue, all-or-nothing JSON request parsing | next stage implements queue, store, or ACK |
| Benchmark profile | valid only for measurement | drop sink, prewarmed cache, relaxed durability, fixed payload corpus | benchmark result is cited outside its profile |
| Deferred | explicitly not decided | ACK commit boundary, JSON partial success, index allow-list syntax | named dependency becomes active |

Current decision gaps that should be normalized into this structure:

- HEC response compatibility for body-too-large, unsupported encoding, bad path, and timeout classes.
- Queue topology and backpressure policy for global, per-source, per-format, per-core, and store-partitioned queues.
- Capture-file format, durable commit boundary, and ACK boundary.
- Parser capability metadata and field alias views for Splunk/CIM, Sigma, ECS, and OTel.
- Benchmark profile definitions for drop, capture, queue, durable store, and indexed store.


---

## 6. Initial Work Sequence

The first sequence keeps product behavior and implementation infrastructure aligned without duplicating the infrastructure spec here.

1. Implement typed configuration, validation, startup, and shutdown.
2. Centralize HEC outcomes, request error mapping, and public message text.
3. Lock raw/event protocol behavior for auth, body, gzip, JSON envelopes, raw line framing, and no-data cases.
4. Create request fixtures for JSON event, raw, gzip, malformed auth, malformed JSON, bad gzip, oversize bodies, and no-data bodies.
5. Implement capture states enough to distinguish accepted, queued, written, flushed, and durable claims.
6. Add bounded queue insertion between `HecEvents` formation and write path once direct capture behavior is stable.
7. Run local curl/process tests, then selected Splunk Enterprise and Vector comparisons.
8. Record benchmark and validation evidence with stable run metadata, stage timing, resource samples, and result files.

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
| Sink commit | Accepted, queued, written, flushed, and durable are different states | Name these states and decide what HTTP success means |
| Sink failure | A sink may fail after request acceptance | Decide whether failure changes health, phase, metrics, or only run ledger |
| Inspection API | Term/time inspection is needed, but exact command/interface is unsettled | Start with one readback helper over capture files |
| Index namespace | `index` is logical first, physical later | Store as event field; defer partitioning |
| Resilience state | Success response, file append, flush, fsync, and ACK durability are distinct | Name the strongest state actually reached by each mode |

---

## 8. References

References here are external comparison points. The documentation map above is the source for project-document placement.

### 8.1 External Comparison Points

1. [Splunk: Format events for HTTP Event Collector](https://docs.splunk.com/Documentation/Splunk/latest/Data/FormateventsforHTTPEventCollector) — JSON envelope and metadata examples.
2. [Splunk: Troubleshoot HTTP Event Collector](https://docs.splunk.com/Documentation/Splunk/latest/Data/TroubleshootHTTPEventCollector) — error/status behavior.
3. [Vector `splunk_hec_logs` sink](https://vector.dev/docs/reference/configuration/sinks/splunk_hec_logs/) — real HEC client behavior, batching, ACK, retry, TLS.
4. [Fluent Bit Splunk output](https://docs.fluentbit.io/manual/data-pipeline/outputs/splunk) — common shipper configuration vocabulary.
5. OpenTelemetry Collector contrib `splunkhecreceiver` — server-side implementation reference.
6. Local Splunk Enterprise — ground truth for selected edge cases when docs and clients disagree.

---

## 9. Appendix — Validation And Benchmark Evidence

This appendix records concrete validation and benchmark evidence: what was mapped, what was run, what broke, what was fixed, and what remains open.

### 9.1 Reporter Component Map

Reporter component/source mapping is now part of the stack design and code path.

| Reporter component | Tracing target | Processing origin | Typical facts |
|--------------------|----------------|-------------------|---------------|
| `Component::Hec` | `hec.receiver` | route, endpoint, request completion | `hec.request.received`, `hec.request.succeeded`, `hec.request.failed` |
| `Component::Auth` | `hec.auth` | authorization header and token checks | `hec.auth.token_required`, `hec.auth.invalid_authorization`, `hec.auth.token_invalid` |
| `Component::Body` | `hec.body` | content length, body read, gzip decode | `hec.body.too_large`, `hec.body.timeout`, `hec.body.gzip_request`, `hec.body.gzip_failed` |
| `Component::Parser` | `hec.parser` | event/raw interpretation | `hec.parser.failed`, `hec.parser.events_parsed` |
| `Component::Sink` | `hec.sink` | `HecEvents` disposition and capture/drop sink | `hec.sink.failed`, `hec.sink.completed` |

Important implementation detail: `tracing` callsite targets must be literals, not dynamic strings. The implementation therefore branches on `Component` and emits through literal targets such as `target: "hec.auth"`. This is mildly repetitive but keeps target-level filtering fast and compatible with `tracing-subscriber::EnvFilter`.

Current filter example:

```sh
HEC_OBSERVE_LEVEL='debug,hec.receiver=debug,hec.auth=debug,hec.body=debug,hec.parser=debug,hec.sink=debug'
```

Convenience TOML such as `[observe.sources] hec.auth = "debug"` is still a design target, not implemented. The currently implemented control is the global `observe.level` filter expression.

### 9.2 Input Coverage Run

Run directory: `/Users/walter/Work/Spank/HECpoc/results/validation-20260505T002004Z`.

Receiver configuration used:

- release binary;
- capture sink enabled;
- max HTTP body bytes `30_000_000`;
- max decoded bytes `60_000_000`;
- max events `1_000_000`;
- JSON tracing enabled;
- console output enabled;
- all component targets set to debug.

Inputs exercised:

| Input | Source path | Endpoint | Expected result | Observed |
|-------|-------------|----------|-----------------|----------|
| syslog sample | `/Users/walter/Work/Spank/Logs/spLogs/laz24_20260310_233030/syslog` first 200 KB | raw | accepted as raw lines | `200` |
| auth sample | `/Users/walter/Work/Spank/Logs/spLogs/laz24_20260310_233030/auth.log` first 200 KB | raw | accepted as raw lines | `200` |
| Apache LogHub | `/Users/walter/Work/Spank/Logs/loghub/Apache_2k.log` | raw | accepted as raw lines | `200` |
| OpenSSH LogHub | `/Users/walter/Work/Spank/Logs/loghub/OpenSSH_2k.log` | raw | accepted as raw lines | `200` |
| Windows LogHub | `/Users/walter/Work/Spank/Logs/loghub/Windows_2k.log` | raw | accepted as raw lines | `200` |
| Vector NDJSON | `/Users/walter/Work/Spank/Logs/vector/vector_log.ndjson` | raw | accepted as raw text lines | `200` |
| Wazuh NDJSON | `/Users/walter/Work/Spank/Logs/wazuh/state.ndjson` | raw | accepted as raw text lines | `200` |
| CSV | `/Users/walter/Work/Spank/Logs/prices.csv` | raw | accepted as raw text lines | `200` |
| CRLF | generated `one\r\ntwo\r\nthree\n` | raw | accepted; CR stripped | `200` |
| embedded NUL | generated `a\0b\n` | raw | accepted; preserved in JSON string escaping | `200` |
| invalid UTF-8 | generated `0xff 0xfe \n valid\n` | raw | accepted through lossy text conversion | `200` |
| blank only | generated LF/CRLF blanks | raw | no data | `400`, `{"text":"No data","code":5}` |
| gzip syslog | gzip of syslog sample | raw | accepted after decode | `200` |
| valid HEC JSON | generated envelope | event | accepted | `200` |
| concatenated HEC JSON | generated two envelopes | event | accepted | `200` |
| missing event | generated JSON without `event` | event | event field required | `400`, code `12` |
| syslog to event endpoint | syslog bytes | event | invalid data format | `400`, code `6` |
| missing auth | generated raw | raw | token required | `401`, code `2` |
| bad token | generated raw | raw | invalid token | `403`, code `4` |
| malformed auth | generated raw | raw | invalid authorization | `401`, code `3` |
| unsupported encoding | generated raw with `Content-Encoding: br` | raw | invalid data / unsupported media | `415`, code `6` |
| advertised oversize | generated raw with huge `Content-Length` | raw | request too large | `413`, code `6` |
| parallel Apache | 32 concurrent requests, 8-way client parallelism | raw | all accepted | all `200` |

Summary stats from `/Users/walter/Work/Spank/HECpoc/results/validation-20260505T002004Z/stats.json`:

```json
{"requests_total":54,"requests_ok":46,"requests_failed":8,"auth_failures":3,"body_too_large":1,"timeouts":0,"gzip_requests":1,"gzip_failures":0,"parse_failures":3,"wire_bytes":6805324,"decoded_bytes":6986001,"events_observed":73813,"events_drop_sink":0,"events_written":73813,"sink_failures":0,"latency_nanos_total":9151439000,"latency_nanos_max":327345000}
```

Capture file readback:

- `/Users/walter/Work/Spank/HECpoc/results/validation-20260505T002004Z/capture.jsonl` contains `73_813` records.
- The capture count matches `events_written`.
- The run produced target-separated tracing records: `hec.receiver`, `hec.auth`, `hec.body`, and `hec.parser` were all observed.

### 9.3 Output, Reporting, Record, Benchmark, And Profile Permutations

Output permutations exercised in this pass:

| Mode | Configuration | Purpose | Outcome |
|------|---------------|---------|---------|
| JSON tracing + console + stats + capture | validation run | verify report fan-out and target mapping under real inputs | worked; component targets observed |
| tracing off + console off + stats on + drop sink | benchmark run | isolate request/raw parsing and stats from output/capture overhead | worked; no request failures from `ab` |
| redacted show-config | prior config validation | verify configured redaction text | worked |
| passthrough show-config | prior config validation | verify explicit secret passthrough mode | worked |

Benchmark/profile run directory: `/Users/walter/Work/Spank/HECpoc/results/bench-profile-20260505T002232Z`.

Payload:

- `/Users/walter/Work/Spank/Logs/loghub/Apache_2k.log`;
- `171_239` bytes;
- `1_999` lines/events per request.

`ab` results:

| Run | Complete | Failed | Requests/sec | Mean request time | Notes |
|-----|----------|--------|--------------|-------------------|-------|
| `ab -n 500 -c 1` | 500 | 0 | `3250.11` | `0.308 ms` | client-side `ab` summary |
| `ab -n 2000 -c 16` | 2000 | 0 | `14930.83` | `1.072 ms` | client-side `ab` summary |

Receiver stats after benchmark:

```json
{"requests_total":2505,"requests_ok":2500,"requests_failed":5,"auth_failures":0,"body_too_large":0,"timeouts":0,"gzip_requests":0,"gzip_failures":0,"parse_failures":0,"wire_bytes":428097500,"decoded_bytes":428097500,"events_observed":5000000,"events_drop_sink":5000000,"events_written":0,"sink_failures":0,"latency_nanos_total":1077444000,"latency_nanos_max":2370000}
```

Interpretation limits:

- These are smoke benchmarks, not capacity claims.
- `ab` reports response throughput, not submitted payload throughput, so byte/sec must be computed from receiver stats and elapsed wall time if needed.
- The run used drop sink and output disabled except stats; capture-file results are intentionally separate.
- The `requests_failed = 5` counter in the benchmark run is unexplained because `ab` reported zero failed requests and detailed error counters remained zero. This needs a focused repro with tracing enabled and stats snapshots before and after warmup/readiness.
- macOS `sample` captured a process report at `/Users/walter/Work/Spank/HECpoc/results/bench-profile-20260505T002232Z/sample-c16.txt`; the run was too short for deep attribution, but it records a physical footprint around 25.5 MB during the sampled interval.

### 9.4 Bugs Fixed During This Pass

| Issue | Symptom | Fix | Regression coverage |
|-------|---------|-----|---------------------|
| Raw byte length after lossy UTF-8 | invalid UTF-8 raw lines stored `raw_bytes_len` after replacement-character expansion, not original byte length | added `Event::from_raw_line_with_len` and passed original byte count from raw parser | `parse_raw::lossy_decodes_non_utf8_without_panic` now checks original byte length |
| Advertised oversize counter missing | huge `Content-Length` returned 413 but `body_too_large` stayed zero | routed advertised oversize through `report_body_error` | `handler::advertised_oversize_increments_body_too_large_counter` |
| Component target design mismatch | docs described per-component filter targets but Reporter emitted all tracing under one target | branched Reporter tracing emission by component with literal targets | validation run observed `hec.auth`, `hec.body`, `hec.parser`, and `hec.receiver` targets |

### 9.5 Obvious Inefficiencies And Poor Implementation Areas

These are observed or strongly suspected from code inspection and the validation runs.

| Area | Current implementation | Risk | Improvement direction |
|------|------------------------|------|-----------------------|
| Raw parser allocation | creates a `String` and full `Event` per line immediately | high allocation rate for large raw batches | introduce `RawEventRef`/batch representation or bytes-backed event until sink/store boundary |
| Raw byte preservation | raw endpoint stores lossy text plus byte length, not original bytes | cannot replay exact binary/log input | add optional raw byte capture or escaped byte field before claiming byte-preserving ingest |
| Capture sink | opens and flushes file per HEC request group under a mutex | poor high-concurrency write behavior | persistent buffered writer or write path with explicit flush policy |
| Reporter field serialization | dynamic fields are collapsed into JSON string for tracing | less useful structured filtering/querying in tracing backend | static fields for hot/common fields or custom `valuable`/JSON layer later |
| Counter labels | counters are flat atomics without reason labels | loses distinction between rejection causes beyond a few coarse counters | introduce bounded reason enums or structured stats snapshot |
| Request failure accounting | benchmark run showed 5 request failures with no detailed counters | possible unclassified failure path or warmup artifact | add `REQUEST_FAILED` reason field and trace failed responses during benchmark repro |
| Benchmark method | `ab` client metrics omit submitted bytes/sec and server CPU | misleading throughput interpretation | compute receiver-side bytes/sec/events/sec from stats deltas and wall time; add `time`, `ps`, `sample`, and later `dtrace`/Instruments recipes |
| Component source config | `observe.level` can express target filters but TOML `[observe.sources]` is not implemented | operator-facing config is less readable | map `[observe.sources]` to an `EnvFilter` directive string during config load |

### 9.6 Methodology Outcomes

Useful method choices from this pass:

- Use result directories under `/Users/walter/Work/Spank/HECpoc/results/` with `summary.tsv`, response bodies, server logs, stats, and manifests.
- Keep real-log raw acceptance separate from HEC JSON event compatibility.
- Verify counters against response matrix; the advertised-oversize bug was visible only because response and stats were compared.
- Run output-heavy validation separately from output-light benchmark/profiling.
- Treat each benchmark as evidence for one configuration, not as a general performance claim.

Method problems to fix:

- The first benchmark script accidentally used bare `wait`, which waited on the server process and required manual cleanup. Future scripts should wait only on explicit short-lived child process IDs.
- Benchmarks should snapshot stats before and after each run, not only at the end.
- Benchmark manifests should record binary hash, git status, command line, payload bytes, payload line count, CPU model, OS, and power mode.
- Validation scripts should become checked-in scripts only after their names, outputs, and failure semantics are stable.

### 9.7 Open Questions

| Question | Why it matters | Suggested next action |
|----------|----------------|-----------------------|
| Why did benchmark stats report 5 request failures? | Failure counters must be trusted before performance claims | rerun with tracing enabled for `hec.receiver=debug`, stats before/after each `ab` stage, and response body capture for non-200s |
| Should unsupported encoding have its own counter/fact? | Current response is correct enough, but observability is coarse | add `BODY_UNSUPPORTED_ENCODING` or map to a decode failure reason |
| Should body-too-large code remain HEC code `6`? | Splunk compatibility may differ | compare local Splunk and Vector client behavior |
| Should raw invalid UTF-8 be accepted lossy, rejected, or byte-preserved? | Search/replay correctness depends on this | document raw text policy and add byte-preserving mode if needed |
| Should blank raw lines be skipped or represented? | Some logs may contain meaningful blank records | compare Splunk raw HEC behavior and decide |
| Should capture success imply flushed or merely written to userspace buffer? | HTTP success semantics and ACK later depend on this | define sink commit state for direct file sink |
| Should per-component filters be TOML sugar or first-class Reporter filters? | `EnvFilter` is good for tracing, not all outputs | implement TOML-to-EnvFilter first; add Reporter filters only when needed |

### 9.8 Pending And Future Work Decomposition

Near-term implementation tasks:

1. Add a validation script that reproduces `/Users/walter/Work/Spank/HECpoc/results/validation-20260505T002004Z` without ad hoc shell editing.
2. Add a benchmark script with explicit stats-before/stats-after snapshots and no bare `wait`.
3. Add `[observe.sources]` TOML support that composes into `observe.level`/`EnvFilter` directives.
4. Add failure reason fields to `REQUEST_FAILED` and counters where coarse counters hide cause.
5. Add `BODY_UNSUPPORTED_ENCODING` reporting or an explicit decode/body reason field.
6. Add capture sink mode with persistent buffered writer and configurable flush policy.
7. Add raw-byte preservation design and tests before claiming replay-grade raw ingest.

Validation tasks:

1. Compare the same response matrix against local Splunk Enterprise HEC.
2. Send with Vector as HEC client into this receiver and inspect request shapes.
3. Run full-size syslog and auth.log with raised limits and record bytes/sec/events/sec.
4. Add gzip expansion tests using both valid high-ratio gzip and malformed gzip.
5. Add slow-body tests to exercise idle and total body timeouts.
6. Add no-auth/malformed-auth/bad-token load tests to validate auth rejection cost and logging volume.

Design tasks:

1. Decide raw text versus raw bytes as an explicit product policy.
2. Decide HEC response compatibility for body-too-large, unsupported encoding, ACK disabled, and raw blank-line behavior.
3. Define sink commit states for drop, capture, flushed, and durable modes.
4. Define stats schema with bounded reason labels before Prometheus or external metrics.
5. Decide whether Reporter should own output routing only, or also a source/fact runtime filter table for non-tracing outputs.

### 9.9 Performance Comparison — Current HECpoc, Earlier Rust Work, And Vendor Signals

The current HECpoc numbers measure a localhost HTTP receiver using Axum/Tokio/Hyper, bounded body read, raw line splitting, and the drop sink. They do not include durable file or database commit, indexing, ACK, TLS, remote network, or Splunk-compatible storage/search costs.

| Source | Workload | Result | Interpretation |
|--------|----------|--------|----------------|
| HECpoc current regular run | Apache raw payload, `171_239` bytes and `1_999` events/request, drop sink, `ab -n 500 -c 1` | about `3,000 req/s`; receiver delta about `6.0M events/s`, `489 MiB/s` | HTTP/request overhead visible; very high event rate only because each request carries many raw lines and sink is drop-only |
| HECpoc current regular run | same payload, `ab -n 2000 -c 16` | about `17,011 req/s`; receiver delta about `33.9M events/s`, `2.7 GiB/s`; one extra body-read failure from AB tail behavior | useful upper smoke signal for in-memory raw splitting, not production ingest capacity |
| HECpoc earlier small-body run | tiny raw body, drop sink, `ab -n 1000 -c 1` | about `11,998 req/s`; `0` server failures | measures request overhead with tiny payload, not parsing throughput |
| HECpoc earlier small-body run | tiny raw body, drop sink, `ab -n 5000 -c 50` | about `38,735 req/s`; `0` server failures | shows the server can answer many tiny local HTTP requests when body work is trivial |
| SpankMax focused parser harness | generated Apache, `100_000` rows, `memchr`, simple tokenizer, null store | about `2.78M events/s`, `362 MiB/s` | cleaner CPU parser/tokenizer/store-null measurement; no HTTP, Axum, auth, body limits, or sink durability |
| SpankMax focused parser harness | real Apache-ish input, `13_628` rows, `memchr`, simple tokenizer, null store | about `1.49M events/s`, `444 MiB/s` | parser harness remains the better place to study parser/tokenizer layout |
| SpankMax focused parser harness | syslog, `9_148` rows, simple tokenizer, null store | about `680k events/s`, `125 MiB/s` | syslog/tokenization path is materially heavier than raw-line HEC splitting |
| Earlier integrated `spank-hec` crate | Axum `Bytes` extractor, mpsc queue, file sender | no comparable retained benchmark artifact found | code is useful design history but not a current performance baseline; it reads whole bodies before HEC-owned limits and uses a different queue/file path |

External vendor signals are not apples-to-apples, but they set order-of-magnitude expectations:

- Splunk Stream HEC HTTP raw tests report `streamfwd` sender rates from `13` to `11,437 events/sec` in a 100K-response HTTP test across 8 Mbps to 10 Gbps, and up to `22,411 events/sec` in a 25K-response HTTP test before drop rates appear at higher rates. Source: [Splunk Independent streamfwd HEC HTTP raw tests](https://help.splunk.com/en/splunk-enterprise/collect-stream-data/install-and-configure-splunk-stream/8.0/splunk-stream-performance-tests-and-considerations/independent-streamfwd-hec-tests---http-raw-events).
- Splunk HEC troubleshooting/performance guidance emphasizes batching, HTTP versus HTTPS, keep-alive, Monitoring Console dashboards, and persistent queue cost rather than a single universal throughput number. Source: [Splunk HEC troubleshooting](https://docs.splunk.com/Documentation/Splunk/9.4.2/Data/TroubleshootHTTPEventCollector).
- Vector sizing guidance estimates unstructured logs at about `10 MiB/s/vCPU` and structured logs at about `25 MiB/s/vCPU`, explicitly as conservative sizing starting points that vary by workload. Source: [Vector sizing and capacity planning](https://vector.dev/docs/setup/going-to-prod/sizing/).
- Vector's public positioning says it is high-performance and claims up to `10x` faster than alternatives, but that is vendor positioning, not a replacement for our local apples-to-apples HEC receiver tests. Source: [Vector GitHub README](https://github.com/vectordotdev/vector).

Current conclusion: HECpoc is already fast enough that the next bottlenecks should be correctness, measurement reliability, and sink/queue design rather than raw Axum/Tokio viability. The parser harness still beats the HEC server as a controlled microbench because it removes HTTP and request lifecycle overhead. Vendor numbers reinforce that batching dominates event/sec comparisons; request/sec without payload size is nearly meaningless.

### 9.10 Five-Failure Diagnosis And Benchmark Tooling

The previous `requests_failed = 5` anomaly was reproduced as the same class with a smaller count: AB completed the configured request count successfully, while the server observed one or more extra tail requests that failed during body read with HEC code `6`. After adding `body_read_errors`, the regular run recorded `requests_ok = 2500`, `requests_failed = 1`, and `body_read_errors = 1`; AB still reported `0` failed requests.

Likely interpretation: ApacheBench can leave extra/incomplete POST attempts near the end of a concurrent run. Hyper delivers those as body read errors after the HEC handler has accepted the route and auth context. These should not be mixed into the measured successful-request throughput; they should be reported separately as client/tool tail behavior unless reproduced with another client.

Instrumentation added:

| Script or field | Path | Purpose |
|-----------------|------|---------|
| `bench_hec_ab.sh` | `/Users/walter/Work/Spank/HECpoc/scripts/bench_hec_ab.sh` | builds release binary, starts HEC receiver, runs AB single/concurrent stages, captures stats before/after, starts system monitor, writes `summary.json` |
| `capture_system_stats.sh` | `/Users/walter/Work/Spank/HECpoc/scripts/capture_system_stats.sh` | samples process CPU, memory, thread count, descriptor count, top output, VM stats, netstat, iostat, and thread listing |
| `analyze_bench_run.py` | `/Users/walter/Work/Spank/HECpoc/scripts/analyze_bench_run.py` | parses AB output and HEC stats deltas into receiver requests/sec, MiB/sec, events/sec, and failure counters |
| `body_read_errors` | `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/stats.rs` | separates malformed/incomplete body stream failures from parser/auth/timeout failures |
| `failure_reason` | `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/report.rs` | includes the HEC response text in failed request reports |

Default benchmark invocation:

```bash
cd /Users/walter/Work/Spank/HECpoc
scripts/bench_hec_ab.sh
```

Useful overrides:

```bash
PAYLOAD=/Users/walter/Work/Spank/Logs/spLogs/laz24_20260310_233030/syslog C1_REQUESTS=1000 CN_REQUESTS=10000 CONCURRENCY=32 PORT=18450 MONITOR_INTERVAL=2 scripts/bench_hec_ab.sh
```

Long-run policy:

1. Always keep `stats-before.json`, stage stats, AB output, `summary.json`, server logs, and `system/` samples together under one result directory.
2. Compare AB-reported failures with HEC `requests_failed` and reason counters.
3. Treat `body_read_errors` during AB as a benchmark-tool artifact until reproduced with `oha`, `wrk`, Vector, or a raw socket harness.
4. Report both request/sec and event/sec, plus payload bytes/sec. Request/sec alone is misleading when one request can contain one line or two thousand lines.
5. Use drop sink, capture sink, queue sink, and durable sink as separate benchmark modes; never blend them into one headline number.


---

## 10. Appendix — HEC Return Values, Limits, And Constraints

This appendix is the background ledger for tightening HEC compatibility, upgrading tests, and deciding where Spank should intentionally diverge from Splunk. It cross-checks the current receiver implementation against Splunk's published HEC format and troubleshooting pages, then enumerates present bounds and unaddressed edge cases.

Primary external references:

1. [Splunk: Troubleshoot HTTP Event Collector](https://help.splunk.com/?resourceId=SplunkCloud_Data_TroubleshootHTTPEventCollector) — current HEC status-code table, HEC metrics fields, and performance notes.
2. [Splunk: Format events for HTTP Event Collector](https://help.splunk.com/?resourceId=SplunkCloud_Data_FormateventsforHTTPEventCollector) — authentication forms, channel header, event metadata, batch formats, and raw parsing behavior.

Current implementation anchors:

1. `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/outcome.rs` — HEC response body and HTTP status mapping.
2. `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/protocol.rs` — configurable HEC response code defaults.
3. `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/config.rs` — CLI/env/TOML/default limits and validation.
4. `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/body.rs` — advertised length, HTTP body, timeout, and gzip limit handling.
5. `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/parse_event.rs` — `/services/collector/event` JSON envelope parsing.
6. `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/parse_raw.rs` — `/services/collector/raw` line splitting and lossy text conversion.
7. `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/handler.rs` — route handling, health response, and current handler-level tests.

### 10.1 Splunk HTTP Status Cross-Check

Splunk's current troubleshooting table assigns particular meaning to HEC response-code/status pairs. The current receiver produces a smaller but not identical HTTP status set.

| HTTP status | Splunk HEC meaning | Current receiver behavior | Gap or action |
|-------------|--------------------|---------------------------|---------------|
| `200 OK` | code `0` success; code `17` healthy; codes `24`/`25` approaching queue/ACK capacity | success uses `200`/code `0`; health uses `200`/code `17` when serving | success path exists; health is minimal; queue/ACK warning statuses not implemented |
| `400 Bad Request` | codes `5`, `6`, `7`, `10`, `11`, `12`, `13`, `14`, `15`, `16`, `21`, `22` | no data, invalid JSON/data, missing/blank event, indexed-field handling use `400`; unknown index/channel/ACK/query-auth states absent | add compatibility tests for implemented cases; decide whether unsupported features return Splunk-specific codes or remain absent |
| `401 Unauthorized` | code `2` token required; code `3` invalid authorization | implemented for absent/blank auth and malformed auth | covered partly by handler tests; add response-body tests for invalid authorization |
| `403 Forbidden` | code `1` token disabled; code `4` invalid token | invalid token implemented; disabled token absent | token store has no disabled-token state; add if token metadata appears |
| `500 Internal Error` | code `8` internal server error | no explicit HEC internal-error outcome | add only when real sink/runtime failures need conversion to HEC JSON rather than process failure |
| `503 Service Unavailable` | code `9` server busy; codes `18`/`19`/`20` unhealthy; code `23` shutting down | server busy is used for health admission failure, event-count limit, and sink failure; health unhealthy returns `503` with code `18` | split busy, queue-full, shutting-down, and ACK-unavailable when those states exist |
| `429 Too Many Requests` | code `26` queue at capacity; code `27` ACK channel at capacity | not generated | preferred future mapping for hard queue saturation, instead of overloading `503`/code `9` |
| `408 Request Timeout` | not listed in Splunk HEC table | body idle/total timeout maps to `408`/code `9` | defensible HTTP, but Splunk-incompatible unless local Splunk proves similar behavior; add slow-client tests and decide |
| `413 Payload Too Large` | not listed in current Splunk HEC table | advertised or actual over-limit body maps to `413` with HEC code `6` | defensible HTTP, but Splunk-incompatible; test local Splunk before deciding whether to map to `400`/`6`, `429`/`26`, or keep `413` |
| `415 Unsupported Media Type` | not listed in Splunk HEC table | unsupported `Content-Encoding` maps to `415` with HEC code `6` | defensible HTTP, but Splunk-incompatible; test local Splunk with `br`, malformed gzip, and bad content-type/header values |
| `404 Not Found` | incorrect path | Axum will return default `404` for unregistered paths, not a deliberate HEC JSON outcome | add explicit 404 route if compatibility requires JSON body or metrics for incorrect URL |

Important conclusion: the current receiver is HEC-shaped but not yet Splunk-status-compatible for all failure classes. The strongest mismatch candidates are body too large, unsupported encoding, timeout, unhealthy health status, and incorrect-path handling.

### 10.2 HEC Return Code Coverage

The current Splunk table lists HEC codes `0` through `27`, with some gaps in feature coverage rather than just missing tests. Current receiver protocol defaults expose only codes `0`, `2`, `3`, `4`, `5`, `6`, `9`, `12`, `13`, `15`, `17`, `18`, and `23`.

| HEC code | Splunk status/message | Current implementation | Current test/validation coverage | Gap or action |
|----------|-----------------------|------------------------|----------------------------------|---------------|
| `0` | `200 Success` | `HecResponse::success` | validation run observed successful requests; no focused unit assertion on success JSON | add handler success body test for raw and event endpoints |
| `1` | `403 Token disabled` | absent | none | add only with token metadata/disabled state |
| `2` | `401 Token is required` | `HecError::TokenRequired` | handler unit test asserts `401` body with code `2`; validation covers missing auth | covered; add blank-header handler test if desired |
| `3` | `401 Invalid authorization` | `HecError::InvalidAuthorization` | auth unit tests cover parser error; validation covers malformed auth; no handler body unit assertion | add handler test for malformed auth response body |
| `4` | `403 Invalid token` | `HecError::InvalidToken` | auth unit test covers token-store error; validation covers bad token; no handler body unit assertion | add handler test for invalid-token response body |
| `5` | `400 No data` | `HecError::NoData` | raw unit covers blank-only body; validation covers blank-only raw; no event empty handler body test | add handler tests for empty event/raw body and blank raw body |
| `6` | `400 Invalid data format` | malformed JSON/body stream/content-length/gzip decode; also reused for unsupported encoding and body too large | parser unit covers trailing garbage; body unit covers malformed content length and limits; validation covers syslog-to-event, unsupported encoding, oversize | split tests by reason and decide whether body-too-large/unsupported-encoding should keep code `6` |
| `7` | `400 Incorrect index` | absent; `index` stored but not validated | none | requires index allow-list/token policy before implementation |
| `8` | `500 Internal server error` | absent | none | define when sink/runtime failures should become code `8` instead of `9` |
| `9` | `503 Server is busy` | max-event limit, health not admitting work, sink failure, timeout uses code `9` | parse-event unit covers event-count limit; timeout not covered; handler sink/health busy not covered | add slow-body tests; add health and sink-failure response tests; reconsider mapping of timeout |
| `10` | `400 Data channel is missing` | absent | none | needed when ACK/channel semantics implemented |
| `11` | `400 Invalid data channel` | absent | none | needed when ACK/channel semantics implemented |
| `12` | `400 Event field is required` | missing or `null` `event` | parser unit and handler unit cover missing event; parser unit covers null event | covered for JSON endpoint; add array-batch variant after array support decision |
| `13` | `400 Event field cannot be blank` | empty string event | parser unit covers blank event | add handler response-body test |
| `14` | `400 ACK is disabled` | absent | none | implement only if `/ack` endpoint or ack query path is added |
| `15` | `400 Error in handling indexed fields` | `fields` absent ok; `fields` must be object; object/array values rejected | parser unit covers nested object; no tests for `fields` array, scalar top-level, null, or scalar values | expand indexed-field validation tests and compare Splunk behavior |
| `16` | `400 Query string authorization is not enabled` | absent | none | current auth ignores `?token=` entirely; add explicit behavior if query auth is considered |
| `17` | `200 HEC is healthy` | health endpoint uses configured healthy code | handler tests cover healthy response | covered for serving phase |
| `18` | `503 HEC unhealthy, queues full` | health endpoint uses configured unhealthy code for starting/non-ready phase | handler tests cover starting/unhealthy response | refine into queue-full once bounded queue state exists |
| `19` | `503 HEC unhealthy, ACK unavailable` | absent | none | add only with ACK service |
| `20` | `503 HEC unhealthy, queues full and ACK unavailable` | absent | none | add only with queue + ACK state composition |
| `21` | `400 Invalid token` | absent as a separate endpoint/config-token-management code | none | probably token-management endpoint specific; do not implement until endpoint exists |
| `22` | `400 Token disabled` | absent as a separate endpoint/config-token-management code | none | probably token-management endpoint specific; do not implement until endpoint exists |
| `23` | `503 Server is shutting down` | `Phase::Stopping` maps health and ingest admission to code `23` | handler tests cover health and ingest request while stopping | covered for explicit phase; still needs graceful-drain system test |
| `24` | `200 HEC queue approaching capacity` | absent | none | add only after queue occupancy thresholds exist |
| `25` | `200 HEC ACK approaching capacity` | absent | none | ACK-specific |
| `26` | `429 HEC queue at capacity` | absent | none | better future hard-backpressure mapping than current event-count `503` overload |
| `27` | `429 HEC ACK channel at capacity` | absent | none | ACK-specific; earlier local notes associating `27` with request-size behavior must be retired or verified against local Splunk before reuse |

Actionable distinction:

- Implemented and handler-tested now: `2`, `12`; partially `6` and body-size status.
- Implemented and unit/validation-covered but not handler-body-covered: `3`, `4`, `5`, `9`, `13`, `15`.
- Implemented and handler-tested: health codes `17`, `18`, and shutdown code `23`.
- Present as code paths but weakly invokable or untested at handler level: timeout, health unhealthy, sink failure, unsupported encoding body, body too large body.
- Not addressed by design yet: disabled tokens, index allow-list, ACK/channel, query-string auth, queue capacity states, internal-error state, incorrect-path metrics/body.

### 10.3 Size, Transfer, And Buffer Bounds

Current configured values and constraints:

| Bound | Default | Validation | Current enforcement |
|-------|---------|------------|---------------------|
| `hec.addr` | `127.0.0.1:18088` | valid socket address; port must be greater than zero | listener bind at startup |
| `hec.token` | `dev-token` | non-empty; no ASCII control characters | exact token membership in `TokenStore` |
| `hec.capture` | none | if present, cannot be empty | file sink path when capture sink is wired |
| `limits.max_bytes` | `1_048_576` | must be greater than zero | maximum advertised `Content-Length` and maximum received HTTP body bytes |
| `limits.max_decoded_bytes` | `4_194_304` | must be at least `max_bytes` | maximum identity body after receipt and maximum gzip-expanded body |
| `limits.max_events` | `100_000` | must be greater than zero | maximum parsed events per request for raw and event endpoints |
| `limits.idle_timeout` | `5s` | must be greater than zero | maximum wait for next body frame |
| `limits.total_timeout` | `30s` | must be at least idle timeout | maximum complete body-read duration |
| `limits.gzip_buffer_bytes` | `8_192` | `512..=1_048_576` | scratch buffer used by gzip decoder |
| `observe.level` | component target expression | must parse as `tracing_subscriber` targets | tracing filter expression |
| `observe.format` | `compact` | one of `compact`, `json` | tracing formatter selection |
| `observe.redaction_mode` | `redact` | one of `redact`, `passthrough` | config rendering redaction |
| `observe.redaction_text` | `<redacted>` | non-empty | config rendering substitute |
| `observe.tracing` | `true` | boolean | tracing output enabled/disabled |
| `observe.console` | `false` | boolean | console report output enabled/disabled |
| `observe.stats` | `true` | boolean | stats counter updates enabled/disabled |

Current hard or implicit limits:

- Partial HTTP headers do not reach HEC code and therefore do not use `limits.idle_timeout` or `limits.total_timeout`; header timeout/header-size policy requires owned Hyper/hyper-util serving or a front proxy.
- No independent per-line maximum exists for raw input; a single raw line may consume almost the whole decoded body cap.
- No independent JSON nesting, string length, field count, metadata length, token length maximum, or index/source/sourcetype length maximum exists.
- No accepted-connection count, concurrent-request count, per-peer byte rate, or per-peer failure-rate limit exists yet.
- No configured queue capacity exists yet because enqueue/dequeue is not wired as the core path from `HecEvents` to write path.
- No explicit read buffer size is exposed beyond Axum/Hyper/Tokio internals and the gzip scratch buffer.
- No filesystem flush, `fsync`, rotation, capture-file size, or disk-available bound exists for capture mode.

Immediate upgrade candidates:

1. Add explicit body-limit compatibility decisions for `413` versus Splunk table mappings.
2. Add independent raw-line length and JSON-depth/field-count limits before accepting adversarial production traffic.
3. Add connection/request concurrency and per-source accounting before meaningful DoS-resilience claims.
4. Add sink/capture bounds: max file size, flush policy, sync policy, write timeout, and failure mapping.

### 10.4 Syntax, Punctuation, Separators, And Endpoint Shape

Current accepted request and payload syntax:

| Area | Current behavior | Splunk comparison | Gap or action |
|------|------------------|-------------------|---------------|
| Authorization header | accepts `Splunk <token>` and `Bearer <token>`, case-insensitive scheme; rejects absent, non-text, unknown scheme, missing token | Splunk documents `Authorization: Splunk <hec_token>` plus basic auth and query-string auth | decide whether `Bearer` is intentional extension; basic and query auth absent |
| Basic auth | absent | Splunk accepts token as password in basic auth form | add only if compatibility tests require common clients using `-u x:token` |
| Query-string auth | absent | Splunk supports `?token=` only when enabled | add explicit disabled response code `16` if recognized but disallowed |
| Channel header/query | ignored | required for raw requests when ACK is enabled | safe while ACK is absent; must become explicit once ACK appears |
| Content-Encoding | accepts absent, empty, `identity`, `gzip`; rejects other values | Splunk behavior must be tested for unsupported encodings | add local Splunk comparison for `br`, mixed encodings, and malformed header bytes |
| Content-Length | malformed value returns invalid-data; over-limit advertised length returns body-too-large before body read | Splunk status mapping uncertain from docs | add local Splunk comparison |
| JSON event endpoint | accepts concatenated JSON objects deserialized as `HecEnvelope`; missing/null `event` rejected; empty string `event` rejected | Splunk documents the batch protocol as event objects stacked one after another, not a JSON array | current concatenated-object support matches the documented batch shape; JSON array batches should be rejected unless a specific client proves they are needed |
| Event value type | string stored directly; non-string JSON converted with `to_string()` | Splunk says event data can be string, number, object, and so on | acceptable for initial capture; downstream store may need original JSON type preservation |
| `fields` value | must be object; nested object/array values rejected; scalar and null values accepted | Splunk requires a flat object for indexed fields; raw endpoint `fields` not applicable | add tests for scalar field values, null, array top-level, object top-level, and raw endpoint with `fields` query/body |
| Raw endpoint splitting | splits on LF, strips one trailing CR, skips blank lines | Splunk applies line-breaking rules and can use sourcetype/props; raw events must not span requests | current behavior is simpler than Splunk and not source-type aware |
| Incorrect HEC paths | default Axum 404 for HEC-looking but unregistered paths such as `/services/collector/rawx`, `/services/collector/ack`, or `/services/collector/event/2.0` | Splunk metrics include incorrect URL requests | add deliberate `/services/collector/*path` fallback if compatibility/metrics matter |

Punctuation and separators currently have narrow meaning:

- LF (`\n`) is the only raw event separator.
- A single CR before LF is stripped; interior CR is preserved.
- NUL (`\0`) is preserved inside raw event strings and escaped by JSON serialization in capture/output.
- Quotes, braces, brackets, parentheses, and commas have no meaning on raw endpoint.
- JSON endpoint punctuation is entirely governed by `serde_json`; malformed, unterminated, or trailing non-whitespace input produces invalid-data at the relevant zero-based event index.

### 10.5 Character Set, Encoding, And Escaping

Current character handling:

| Stage | Current behavior | Consequence |
|-------|------------------|-------------|
| HTTP headers | `HeaderValue::to_str()` requires valid visible header text; non-text auth or encoding headers are rejected | good guard against malformed header bytes; not tolerant of arbitrary byte tokens |
| Wire body | collected as bytes; no charset header is interpreted | HEC payload policy is endpoint-specific rather than HTTP charset-specific |
| Gzip body | decoded using `flate2::read::GzDecoder` with output cap | malformed gzip maps to invalid-data; gzip bombs capped by decoded-byte limit |
| JSON event body | `serde_json::Deserializer::from_slice` requires valid JSON bytes, effectively UTF-8 JSON text | invalid UTF-8 in JSON endpoint is invalid data |
| Raw body | each line uses `String::from_utf8_lossy` | invalid UTF-8 is accepted with replacement characters; exact input bytes are not replayable from stored raw string |
| Capture output | JSON serialization escapes control characters as needed | NUL/control characters should not crash output, but byte-for-byte replay is not guaranteed |

Open policy decisions:

1. Raw endpoint should choose one of three explicit modes: strict UTF-8 reject, lossy text accept, or byte-preserving accept with separate display conversion.
2. JSON endpoint should stay strict JSON unless compatibility tests prove Splunk accepts non-standard encodings.
3. Header token policy should decide whether tokens are strictly visible text or arbitrary opaque bytes.
4. Capture output should state whether it preserves semantic event text, JSON value, or exact source bytes.

### 10.6 Incomplete Entries, Quotes, Brackets, And Request Boundaries

Current incomplete-input behavior:

| Input condition | Current outcome |
|-----------------|-----------------|
| Empty body | no data, code `5` |
| Raw body with only blank LF/CRLF lines | no data, code `5` |
| Raw final line without trailing LF | accepted as one event |
| Raw line with unmatched quote/brace/parenthesis | accepted; raw endpoint does not parse structure |
| Raw record split across two HTTP requests | treated as two independent requests/events; no cross-request assembly |
| Event JSON with unterminated string/object/array | invalid data, code `6`, with zero-based `invalid-event-number` |
| Event JSON with one good object then trailing garbage | invalid data, code `6`, `invalid-event-number` points at the failed next item |
| Event JSON array batch | currently not accepted, matching Splunk's documented stacked-object batch shape | add explicit rejection test and only change if local Splunk or a target shipper requires arrays |
| Malformed gzip or truncated gzip | invalid data, code `6` |
| Slow or stalled body | timeout maps to HTTP `408`, HEC code `9` |
| Partial HTTP headers | handled before HEC handler by Axum/Hyper/OS behavior; no HEC-configured timeout yet |

Key compatibility issue: Splunk explicitly says raw events must be contained within a single HTTP request and cannot span multiple requests. The current receiver matches that boundary, but not Splunk's sourcetype-driven line-breaking sophistication.

### 10.7 Event Granularity, Sections, Attributes, And Alignment

Current event granularity:

- `/services/collector/event`: one JSON envelope becomes one internal `Event`; concatenated envelopes become multiple `Event` values.
- `/services/collector/raw`: each non-empty LF-delimited line becomes one internal `Event`.
- Capture mode writes one JSON record per internal `Event`.
- Metadata fields `time`, `host`, `source`, `sourcetype`, `index`, and `fields` attach to the corresponding event envelope or raw-derived event.
- Raw endpoint request-level metadata through query string is not implemented.
- No carry-forward metadata state exists between event envelopes.
- No binary or memory alignment guarantee exists at the event API boundary; storage layout optimization is deferred to the queue/store design.

Splunk comparison:

- Splunk documents optional metadata keys and says omitted values fall back to token/platform defaults.
- Splunk documents `fields` as flat indexed fields only for the event endpoint.
- Splunk documents concatenated JSON object batches and explicitly distinguishes them from JSON arrays.
- Splunk raw parsing can use timestamp and sourcetype rules rather than simple LF splitting.

Implementation implications:

1. Keep concatenated-object batch tests as the primary event-endpoint compatibility proof.
2. Add a negative JSON-array test unless local Splunk accepts arrays in this endpoint/version.
3. Decide whether raw query parameters (`host`, `source`, `sourcetype`, `index`, `channel`) become request metadata.
4. Preserve original endpoint and original field names while storing canonical internal names.
5. Keep queue/store alignment decisions out of HEC parsing until the internal event batch representation is designed.

### 10.8 Numeric, Datetime, Sequence, And Code Representation

Current numeric handling:

| Value | Current representation | Constraint or gap |
|-------|------------------------|-------------------|
| `time` metadata | JSON number or string parsed to `f64`; other types become `None` | no range check; no precision policy; invalid value is silently dropped rather than rejected |
| `raw_bytes_len` | `usize` from original line/string length depending on endpoint | raw invalid UTF-8 preserves original byte length; JSON string length is UTF-8 byte length after JSON parsing |
| HEC response `code` | `u16`, configurable for implemented protocol fields | no validation that configured code belongs to Splunk table or matches HTTP status |
| `ackId` | optional `u64` field in response type | not assigned; ACK not implemented |
| `invalid-event-number` | zero-based `usize` | parser tests depend on zero-based index; compare local Splunk if exact behavior matters |
| stats counters | atomic unsigned counters | no reason-label taxonomy beyond current fields |
| durations/latency | nanoseconds in stats totals/max | current external benchmark interpretation needs wall-clock delta and explicit units |

Datetime decisions to make:

1. Keep `time` as floating seconds for Splunk compatibility at the HEC boundary.
2. Convert to internal integer nanoseconds or microseconds only after defining precision, rounding, and out-of-range behavior.
3. Reject invalid `time` only if local Splunk does; otherwise record a parser warning/fact while accepting the event.

Protocol-code decisions to make:

1. Validate configured protocol codes against the Splunk table, or explicitly allow compatibility overrides.
2. Stop using code `17` for unhealthy health responses.
3. Stop overloading code `9` for unrelated conditions once queue, timeout, shutdown, and sink-failure states are separated.

ACK design decisions already settled enough to guide implementation:

- ACK is scoped to the HTTP request/batch, not to each row/event/line.
- A request containing many raw lines or stacked JSON event objects should receive at most one `ackId`.
- ACK boundary should be configurable for explicit modes such as `enqueue`, `write`, `flush`, `fsync`/`db_commit`, and later `indexed`.
- `enqueue` is acceptable for benchmark/load-test mode only when labeled as such; production ACK should wait for a real durable boundary.
- ACK registry is required before implementing `/services/collector/ack`: channel map, per-channel IDs, pending status table, capacity limits, idle cleanup, and consumed status removal.

### 10.9 Test Upgrade Plan From This Appendix

Focused tests to add next:

| Test group | Cases |
|------------|-------|
| Handler response matrix | success raw/event; invalid authorization; invalid token; no data; blank event; indexed-field error; unsupported encoding body; body too large body; timeout; incorrect path |
| Splunk compatibility probes | local Splunk response for oversized body, unsupported encoding, timeout/slow body, JSON array rejection, raw blank lines, basic auth, query auth disabled, missing channel with ACK disabled/enabled |
| JSON parser edges | concatenated-object batch, top-level array rejection, scalar top-level, fields top-level non-object, fields null/scalar/object/array values, invalid UTF-8 JSON, huge strings, nesting depth |
| Raw parser edges | final line no LF, CR-only separators, interior CR, NUL, other C0 controls, invalid UTF-8, very long line, blank-line policy |
| Limit enforcement | advertised length over cap, actual body over cap without content length, gzip expansion over cap, gzip buffer min/max config, event-count cap raw/event |
| Config/protocol safety | invalid protocol code/status pairing if validation added; token length/control; source filter syntax; redaction passthrough |
| Metrics/reporting | each HEC outcome increments an expected bounded counter or reason field; unknown paths counted if explicit 404 handler added |

Implementation priorities implied by this review:

1. Expand tests before changing response mappings; the receiver needs a stable compatibility baseline.
2. Compare the same matrix against local Splunk Enterprise before deciding whether `408`, `413`, and `415` remain intentional extensions.
3. Preserve concatenated-object batches and health-code split because they are direct Splunk-documented behavior and low conceptual risk.
4. Postpone ACK/channel codes until the ACK feature is real; avoid fake compatibility.
5. Add raw-byte policy and per-line limits before claiming production resilience.

### 10.10 Splunk Verification Harness And Development Blockers

This section separates developer decisions from facts that must be discovered against a live Splunk HEC endpoint. It also records what is blocked by missing subsystems rather than by uncertainty.

#### Verify Against Splunk

Run `/Users/walter/Work/Spank/HECpoc/scripts/verify_splunk_hec.sh` with a local Splunk HEC token:

```sh
cd /Users/walter/Work/Spank/HECpoc
SPLUNK_HEC_TOKEN='<token>' \
SPLUNK_HEC_URL='https://127.0.0.1:8088' \
SPLUNK_HEC_INSECURE=1 \
./scripts/verify_splunk_hec.sh
```

The script writes a timestamped result directory under `/Users/walter/Work/Spank/HECpoc/results/` with payloads, response bodies, response headers, curl errors, and `summary.tsv`. It does not assert expected results; Splunk is the oracle for vaguely documented behavior.

Immediate Splunk verification cases:

| Category | Cases | Why now |
|----------|-------|---------|
| Basic event/raw success | normal event, raw lines, final raw line without LF | confirms baseline endpoint and token configuration |
| Documented stacked JSON | `{"event":"one"}{"event":"two"}` | confirms current parser's batch shape matches Splunk |
| Malformed JSON | missing closing brace, missing closing quote, trailing garbage | determines exact code/text/index behavior for parser failures |
| Event required/blank | missing `event`, blank string `event` | validates codes `12` and `13` |
| Indexed `fields` | flat scalar object, nested object, array value, top-level array | determines code `15` boundaries and whether scalar/null values are accepted |
| Raw blank behavior | empty/blank raw body | determines whether Splunk indexes zero events or returns `No data` |
| Oversize/encoding | advertised oversize, unsupported `Content-Encoding`, malformed content length | determines whether our `413`/`415` choices match or intentionally diverge |
| Incorrect HEC path | wrong HEC-looking `/services/collector/...` URL | determines plain Axum-style `404` versus HEC JSON or metric behavior |

Postponed Splunk verification cases:

| Case | Reason for postponement |
|------|-------------------------|
| JSON array batch | Splunk docs describe stacked event objects and distinguish them from JSON arrays; current work should test rejection locally and only enable Splunk probe with `SPLUNK_HEC_RUN_OPTIONAL=1`. |
| Health unhealthy | A healthy local Splunk endpoint is easy to test, but forcing queue-full/ACK-unavailable/shutdown states requires Splunk admin setup and may not be stable across versions. |
| ACK channel states | ACK is explicitly not implemented; missing/invalid channel behavior matters when ACK design starts. |
| Basic/query-string auth | Relevant for compatibility, but not blocking current `Authorization: Splunk <token>` path. |

#### `fields` Test Selection

Immediate local tests:

| Shape | Expected current behavior | Reason |
|-------|---------------------------|--------|
| `fields` flat object with string/number/bool/null values | accept | minimum useful indexed-field compatibility |
| nested object value | reject code `15` | Splunk documents flat indexed fields; nested values create ambiguous indexing semantics |
| array value | reject code `15` | arrays are non-scalar and ambiguous for indexed fields |
| top-level `fields` array/string/null | reject code `15` | `fields` itself must be an object if present |

Postponed `fields` tests:

| Shape | Reason |
|-------|--------|
| raw endpoint `fields` query/body behavior | raw endpoint metadata parsing is not implemented yet |
| index-time verification inside Splunk search results | requires waiting for indexing and SPL search, not just HEC response matching |
| duplicate/colliding field names | requires a canonical field and alias policy |
| extremely many fields or huge field names | belongs with resource-limit and DoS policy after field-count/name-length bounds exist |

#### Conditions And Codes Requiring Future Subsystems

| Code | Condition to detect | Blocking subsystem | First test once available |
|------|---------------------|--------------------|---------------------------|
| `7` incorrect index | event names an index not allowed for token/config | index registry and token-to-index policy | send allowed and disallowed `index` values and verify response/counter |
| `23` shutting down | request arrives after shutdown begins and intake is closed | graceful shutdown orchestration around existing `Phase::Stopping` behavior | begin shutdown, send request during drain, expect `503/code23` or documented alternate |
| `26` queue at capacity | bounded ingest queue cannot accept more work | bounded queue, queue policy, and source/request capacity counters | fill queue with blocked write path, send one more request, expect `429/code26` or chosen busy mapping |
| `27` ACK channel at capacity | ACK-enabled request/batch needs a channel but channel capacity is exhausted | ACK channel registry and capacity policy | create max channels, send new channel, expect `429/code27` or Splunk-matched response |
| `18`/`19`/`20` health subcauses | queue full, ACK unavailable, or both | queue health and ACK health exposed separately | health endpoint under forced queue/ACK conditions |

Current `23` status: implemented at the handler/lifecycle-phase level. `Phase::Stopping` now returns `503/code23` for `/services/collector/health` and for new ingest requests. What remains is a system-level graceful-shutdown test that starts the real server, initiates shutdown, proves new work is rejected with code `23`, and proves already accepted work reaches the selected commit boundary.

#### Backpressure Before Queue Full

Backpressure cannot be validated as queue-full until a bounded queue exists. Interim tests can still exercise earlier admission layers:

1. advertised body too large;
2. actual body too large without `Content-Length`;
3. gzip expansion too large;
4. max events per request;
5. slow body timeout;
6. concurrent request smoke runs with stats deltas.

These prove bounded request processing, not end-to-end queue backpressure. The first real queue-full test should use a bounded queue of size `1`, a write path deliberately blocked on a test latch, and one extra request that must receive a deterministic retryable response.

#### Development Blocker Status

| Area | Status | What is blocked | Next concrete step |
|------|--------|-----------------|--------------------|
| Splunk response gray areas | open | final mapping for `408`, `413`, `415`, unknown route, raw blanks, malformed JSON details | run `scripts/verify_splunk_hec.sh` and record local Splunk version |
| Raw policy | open | replay-grade ingest and binary safety claims | decide strict UTF-8, lossy text, or byte-preserving raw event representation |
| Observability | partially implemented | complete failure reason accounting and benchmark ledger | add bounded reason fields for every `REQUEST_FAILED` path |
| Lifecycle | handler-level `Phase::Stopping` implemented | graceful drain semantics, shutdown request behavior, accepted-work completion | add graceful-shutdown system test harness |
| Index policy | not implemented | code `7`, per-token allowed index validation, logical namespace enforcement | define minimal index allow-list config and behavior for unknown index |
| ACK | postponed, request/batch scoped | codes `10`, `11`, `14`, `19`, `20`, `25`, `27`, `ackId` registry and commit-boundary semantics | implement only after `ack.boundary` and registry design are encoded in config/tests |
| Queue/backpressure | not implemented | code `26`, health queue-full state, source admission policy | add bounded queue and blocked-write-path test |
| Axum accept visibility | deferred | connection counts, peer culling, header timeout tuning, socket backlog/buffers | add task for owned accept loop using `TcpSocket` + Hyper/hyper-util after current HEC matrix stabilizes |

## 11. Appendix — Data-Path Terminology And Stage Definitions

Purpose: name the data component of HECpoc processing from the HTTP request as exposed by Axum/Hyper through HEC validation, batching, queueing, writing, and optional later interpretation. The names should follow function first, and component names second.

### 11.1 Request, Frame, Body, Line, Event, Record, Batch

Use these terms with exact scope:

| Term | Definition | Data | Metadata |
|------|------------|------|----------|
| transport stream | TCP/Tokio/Hyper receive path below the current handler-visible layer | not directly visible in current Axum handler | peer address and connection facts only when exposed by the server stack |
| HTTP request | an HTTP method/path/header/body exchange after HTTP parsing has produced request semantics | headers plus body stream | method, path, query, headers, route match, peer if available |
| HTTP framing | HTTP syntax and transfer structure, not application data | method, URI, headers, chunked/content-length body framing | header parse errors, length hints, keep-alive state |
| HTTP headers | request metadata parsed before body read | header map | authorization, content encoding, content length, HEC channel, source metadata in query params |
| HTTP body | bounded body read before content decoding | body stream chunks accumulated under limits | advertised length, actual read length, body read timing |
| decoded body | HTTP body after content decoding such as gzip | decoded body buffer | decoded length, content encoding, decode errors |
| raw line | one LF-delimited unit from `/services/collector/raw` after raw endpoint line splitting | bytes or text, depending mode | line number, byte offset/length, blank/invalid flags |
| HEC event | one HEC event object or one raw endpoint event candidate | event payload plus HEC metadata | `time`, `host`, `source`, `sourcetype`, `index`, `fields`, token/channel/request id |
| log record | application log structure inside an event, such as syslog, Apache, auditd, JSON Lines, or logfmt | event text or structured event value | parser family, parser variant, parse status/reason |
| `HecEvents` | valid HEC events produced from one HEC HTTP request after endpoint-specific decoding and validation | event vector plus raw references | request id, token/channel, endpoint, event count, body lengths, selected commit state |

`request` means the HTTP request after HTTP processing, not raw `recv()` data. Current Axum/Hyper code does not expose raw `recv()` bytes at the handler layer. If discussing lower-level receive behavior, say `transport stream`; if discussing data visible to HEC code, say `HTTP body`.

`line` exists only where a format or endpoint defines it. HEC `/raw` uses line splitting. HEC `/event` uses JSON envelope boundaries, not newline boundaries. Syslog, Apache, and other file formats may be line-oriented, but multiline parsers can combine several physical lines into one log record.

### 11.2 Functional Data Path

Use this as the active HECpoc data path:

```text
transport stream
  -> HTTP request/framing
  -> HTTP headers and route
  -> auth and request metadata validation
  -> HTTP body
  -> content decode
  -> HEC decode
  -> HEC event validation
  -> HecEvents formation
  -> concrete disposition
  -> selected commit state
  -> optional format interpretation
  -> optional search preparation
```

Short form:

```text
receive HTTP request -> validate headers/auth -> read HTTP body -> content decode -> HEC decode -> validate events -> form HecEvents -> disposition -> commit state -> optional interpretation/search-prep
```

### 11.3 Stage Definitions

| Stage | Function | Input | Output | Extracted / Attached Facts |
|-------|----------|-------|--------|-----------------------------|
| HTTP request/framing | convert transport stream into HTTP request semantics | transport stream below handler | method/path/headers/body stream | peer if exposed, method, path, query, headers, content length, route alias |
| header/auth validation | validate HEC-visible request metadata before reading the full body when possible | HTTP headers and query | accepted request metadata or HEC error | auth scheme/token, channel, content encoding, content length, source query params, route alias |
| body read | enforce HTTP body length and time bounds | HTTP body stream | bounded HTTP body | advertised length, actual read length, idle/total timing, body read error |
| content decode | decode content encoding after enough HTTP body data exists | HTTP body | decoded body | content encoding, decoded length, gzip error, expansion-limit reason |
| HEC decode | decode HEC protocol body | decoded body | HEC event candidates | endpoint kind, raw line number or JSON object number, envelope metadata |
| HEC event validation | validate HEC-visible event requirements | HEC event candidates | valid HEC events or HEC error | missing/blank event, invalid fields, invalid index when configured, invalid event number |
| HecEvents formation | collect valid HEC events produced by one HTTP request | valid HEC events | `HecEvents` | request id, token/channel, endpoint, event count, HTTP body length, decoded length, event payload lengths |
| disposition | choose concrete next action | `HecEvents` | queued/written/dropped/rejected result | disposition kind, queue name if any, write target if any, overflow/busy reason |
| commit state | record strongest completed state | disposition result | response/ACK/reporting state | accepted, queued, written, flushed, durable, indexed |
| format interpretation | parse log-record structure inside events | raw/event payload | parsed record fields | parser family/variant/version, parse status/reason, field aliases |
| search preparation | build search-oriented structures | parsed records or replayable raw events | tokens/columns/index metadata | token counts, field stats, postings/segments when implemented |

### 11.4 Decode, Parse, Normalize, Tokenize

Use `decode` for protocol and representation conversion:

- `content decode`: gzip or other content encoding to decoded bytes.
- `HEC decode`: decoded HEC bytes to HEC event candidates.

Gzip content decode requires HTTP body data. It cannot complete before the relevant body bytes are available. Header validation can reject unsupported `Content-Encoding` before reading the body, but actual gzip validation and expansion-limit enforcement occur while reading/decoding the body.

Use `parse` for log-record structure inside an event:

- syslog prefix and message parse;
- Apache/Nginx access or error parse;
- auditd key/value parse;
- JSON Lines or logfmt parse.

Use `normalize` for canonical field/value mapping after parsing:

- field aliases such as `clientip` to `client_ip`;
- timestamps into one time representation;
- IP/port/status-code typed values.

Use `tokenize` for search-preparation terms:

- field-aware terms;
- URI/path terms;
- position/proximity terms if enabled.

### 11.5 Splunk Functional Stages And Queues

Splunk's public data-pipeline model is `Input -> Parsing -> Indexing -> Search`. Splunk documentation states that the parsing function actually consists of parsing, merging, and typing pipelines. Operational queue names expose buffers between these functions.

| Splunk Queue / Pipeline | Between What And What | Typical Function | HECpoc Interpretation |
|-------------------------|-----------------------|------------------|-----------------------|
| input segment | source acquisition before parsing queue | consume external data, split into blocks, annotate source-level metadata | HTTP request/framing and body read |
| `parsingQueue` | input processors -> parsing pipeline | UTF-8/encoding, line breaker, data-header recognition | content decode and endpoint-specific event boundary work |
| parsing pipeline | consumes `parsingQueue` | line breaking and data-header processing | HEC decode for HEC input; raw line splitting for `/raw` |
| `aggQueue` | parsing pipeline -> merging/aggregation pipeline | queue before aggregator/line-merging work | relevant to multiline file/TCP input, less central to HEC `/event` |
| merging / aggregation pipeline | consumes `aggQueue` | line merging, timestamp extraction, event boundary refinement | future multiline/event breaker work for file/TCP inputs |
| `typingQueue` | merging pipeline -> typing pipeline | queue before regex/typing work | future format interpretation and metadata transforms |
| typing pipeline | consumes `typingQueue` | regex replacements, annotations such as `punct`, metadata transforms | format parse/normalize stage if implemented before storage |
| `indexQueue` | typing pipeline -> indexing pipeline | parsed events waiting to be indexed | queue before durable write/search-prep |
| indexing pipeline | consumes `indexQueue` | output routing, index file/rawdata write, metrics | store write, durable commit, optional search-prep |

`aggQueue` is a real Splunk operational queue name, not just a conceptual drawing. For HECpoc, do not copy the four queues mechanically. Copy the functional lesson: put buffers only where they express a measured control boundary or a required guarantee.

Splunk `header` in this context is data-header recognition during parsing, not HTTP header parsing. HECpoc HTTP headers belong to `HTTP request/framing` and `header/auth validation`.

### 11.6 Splunk-Compatible Metrics To Consider

Current HECpoc metrics should eventually map to Splunk-compatible or Splunk-comparable counters where useful:

| Splunk-Compatible Area | Candidate HECpoc Metric |
|------------------------|-------------------------|
| requests received | `hec.requests_total{endpoint,status,outcome}` |
| incorrect URL | `hec.requests_incorrect_url_total` |
| auth failures | `hec.auth_failures_total{reason}` for missing token, malformed auth, invalid token, disabled token |
| HTTP body received | `hec.http_body_bytes_total{endpoint}` |
| decoded body | `hec.decoded_body_bytes_total{encoding}` and `hec.decode_errors_total{reason}` |
| events received | `hec.events_total{endpoint,outcome}` |
| HEC decode errors | `hec.hec_decode_errors_total{reason}` |
| format parse errors | `hec.format_parse_errors_total{family,reason}` |
| queue pressure | `hec.queue_depth`, `hec.queue_full_total`, `hec.queue_wait_seconds` |
| blocked pipeline | `hec.pipeline_blocked_total{stage}` or `hec.pipeline_blocked_seconds{stage}` once real queues exist |
| ACK state | `hec.ack_missing_channel_total`, `hec.ack_invalid_channel_total`, `hec.ack_pending`, `hec.ack_capacity_total`, `hec.ack_poll_total{result}` |
| output/store errors | `hec.store_write_errors_total`, `hec.store_flush_errors_total`, `hec.store_commit_errors_total` |
| throughput by metadata | `hec.events_by_metadata_total{host,source,sourcetype,index}` only if cardinality policy permits |

`num_of_requests_to_incorrect_url` is a Splunk-documented HEC introspection counter. Use our own metric name, but preserve the concept.

Additional non-Splunk-specific HECpoc metrics:

| Area | Candidate HECpoc Metric |
|------|-------------------------|
| body timeouts | `hec.body_idle_timeouts_total`, `hec.body_total_timeouts_total` |
| body limits | `hec.http_body_too_large_total`, `hec.decoded_body_too_large_total` |
| gzip expansion | `hec.gzip_expansion_ratio` summary/histogram |
| HEC event grouping | `hec.hec_events_count` and `hec.hec_events_decoded_body_bytes` histograms |
| latency by stage | `hec.stage_duration_seconds{stage}` |
| response mapping | `hec.responses_total{http_status,hec_code}` |
| commit state | `hec.commit_state_total{state}` |
| current concurrency | `hec.requests_in_flight`, later `hec.connections_current` when the accept loop is visible |

### 11.7 Vector Architecture Terms And Code Signals

Vector's public architecture is component based:

```text
source -> transform(s) -> sink
```

Vector does not fully describe every internal scheduling and queue boundary in the public architecture documents. The local source shows useful implementation concepts:

| Vector Term / Code Signal | Meaning | HECpoc Lesson |
|---------------------------|---------|---------------|
| source | component that receives data | comparable to HEC receiver, file input, TCP/UDP receiver |
| transform | component that mutates, parses, filters, routes, or enriches events | comparable to format interpretation and search-prep stages |
| sink | component that delivers events to an output | use `sink` for file/capture/drop/transmit/DB output components |
| buffer | sink-side or component-side staging with `when_full` policy | distinguish byte buffers from bounded queues |
| `when_full` | `block`, `drop_newest`, or overflow-style behavior | make full-policy explicit, not implicit |
| batch config | sink batching by event count, byte size, and timeout | define HECpoc batch policy by source request first, then sink coalescing if needed |
| acknowledgements | delivery status flows back to source when enabled | do not claim ACK success before the configured commit state |

Vector both processes individual events and forms outbound batches. Its Splunk HEC source decodes incoming request bodies into individual `Event` values. Its Splunk HEC sink maps input events, partitions where needed, batches by timeout/byte-size/event-count settings, then builds outbound HEC HTTP requests. Local code anchors:

- `/Users/walter/Work/Spank/sOSS/vector/src/sources/splunk_hec/mod.rs` — `EventIterator` yields individual `Event` values from HEC input.
- `/Users/walter/Work/Spank/sOSS/vector/src/sinks/splunk_hec/logs/sink.rs` — `.batched_partitioned(...)` creates outbound sink batches.
- `/Users/walter/Work/Spank/sOSS/vector/src/sinks/splunk_hec/logs/encoder.rs` — encodes `Vec<HecProcessedEvent>` into an outbound HEC body.

HECpoc should not copy Vector's exact batch defaults. The useful lesson is that inbound HEC request grouping and outbound/store aggregation are separate mechanisms.

### 11.8 HECpoc HEC Events And Aggregation Terms

Approved naming direction:

- Use `HecEnvelope` for one decoded JSON object from `/services/collector/event`.
- Use `HecEvents` for valid HEC events after HEC decode and HEC validation.
- Use `RequestRaw` for the decoded `/services/collector/raw` HTTP body before raw line splitting.
- Use `RawEvents` for raw endpoint events after LF splitting.
- Use `Batch` only to describe the HEC HTTP input structure or explicit HEC sender batching, where Splunk already uses the term.
- Use `ParseBatch` only if format parsing is actually performed on a grouped set of events. Otherwise parse events individually.
- Use `WriteBlock` for store/output aggregation for now, because output granularity should not inherit whatever grouping the shipper happened to send. Revisit this name when designing a generalized `Store` interface that must cover append-only files, segment files, SQLite/DuckDB-style databases, and future search-prep storage.

| Term | Formation Rule | Use |
|------|----------------|-----|
| `HecEnvelope` | one JSON object decoded from `/services/collector/event` | HEC event endpoint input structure |
| `HecBatch` | multiple `HecEnvelope` objects stacked in one HEC HTTP request | Splunk-compatible HEC batch terminology only |
| `RequestRaw` | decoded `/raw` HTTP body before LF splitting | raw endpoint input structure |
| `RawEvents` | non-empty raw events produced by LF splitting `RequestRaw` | HEC events for raw endpoint |
| `HecEvents` | valid HEC events from either `HecEnvelope` or `RawEvents` | common post-validation representation |
| `ParseBatch` | explicit group selected for format parsing | only if parser design groups events for CPU/cache behavior |
| `WriteBlock` | store/output aggregation unit selected for append/write efficiency | current preferred term for file/store output grouping |
| `Segment` | durable/search-prep storage unit with metadata | later store/search layout |
| `Chunk` | byte-range subdivision inside a file or segment | low-level storage/corruption/replay unit |
| `Transaction` | database commit group | DB-backed sink/store only |
| `FlushGroup` | group whose buffered writer flush is tracked together | file-output buffering only |

Initial rule: keep request provenance, not request granularity. Later write/store/parse stages may coalesce or split events independently of the original HEC request, but must retain request id, event ordinal, endpoint, token/channel, source metadata, and original body/reference information.

Format parsing should start event-by-event unless a parser or benchmark demonstrates a benefit from `ParseBatch`. If grouped parsing is introduced, `ParseBatch` must name its policy: by sourcetype, byte range, event count, time window, store segment, or CPU/cache partition.

### 11.9 Disposition And Capacity Terms

Avoid vague `admission decision` and `handoff`. Name the concrete disposition:

| Disposition | Meaning |
|-------------|---------|
| reject request | no valid `HecEvents` unit is produced; HEC response reports the reason |
| enqueue HecEvents | `HecEvents` entered a bounded queue |
| write HecEvents | `HecEvents` was written by the configured sink path |
| drop HecEvents | `HecEvents` intentionally discarded in explicit drop/benchmark mode |
| forward HecEvents | `HecEvents` sent to an external destination |
| busy | temporary inability to accept work because a dependent resource is saturated or unavailable |
| full | a specific bounded queue/buffer/capacity limit is reached |

Use `full` for the measured condition and `busy` for the client-facing or aggregate state. Example: `ingest_queue_full` may map to HEC `server busy`.

### 11.10 Commit-State Requirement

Truthful commit reporting means: the response, ACK, metric, and log may not claim a state stronger than what actually completed.

| State | Completed Work | Allowed Claim |
|-------|----------------|---------------|
| accepted | HEC event validation passed | accepted only; not queued, written, or durable |
| queued | bounded queue insert succeeded | queued |
| written | write call returned | written; not durable |
| flushed | userspace flush returned | flushed; not crash durable |
| durable | `fsync`, database commit, or equivalent durable boundary completed | durable |
| indexed | durable evidence and search-prep structures completed | query-ready/indexed |

This is separate from Splunk conformance. Splunk compatibility asks whether the selected external behavior matches Splunk. Commit-state truthfulness asks whether HECpoc's own reported state is technically true.

### 11.11 Enforcement Proposal

If approved, apply these rules across active documents and new code:

1. Use function names before component names: `HEC decode`, `HecEvents formation`, `enqueue HecEvents`, `write HecEvents`, `format interpretation`, `search preparation`.
2. Avoid generic `worker` or `processor` in design names unless the implementation is truly about scheduling rather than function.
3. Replace `admission decision`, `handoff`, and `sink boundary principle` with concrete terms from this appendix.
4. Use `decode` for content/HEC representation conversion and `parse` for log-format interpretation.
5. Use `Batch` only for HEC HTTP input batching or `ParseBatch`; use `WriteBlock` for store/output aggregation until a generalized `Store` interface design chooses a more precise term.
6. Use `full` for a specific capacity and `busy` for the external or aggregate condition.
7. Do not introduce `sealed block` into common terminology until store block layout exists.
8. Every success, ACK, stored, or committed claim must name the commit state it actually reached.

### 11.12 Visual Reference Candidates

Use visuals to reduce repeated prose, not to create another status layer. Good candidates:

| Visual | Purpose | Best Location |
|--------|---------|---------------|
| Stage flow diagram | Show `HTTP request` through `HecEvents`, disposition, commit state, and optional interpretation/search preparation | `HECpoc.md` Appendix 11 |
| Terminology crosswalk | Compare Splunk, Vector, and HECpoc terms without forcing identical architecture | `HECpoc.md` Appendix 11 |
| Commit-state ladder | Prove which response/ACK/log claims are allowed at accepted, queued, written, flushed, durable, and search-ready states | `Store.md` or `HECpoc.md` Appendix 11 |
| `HecBatch` vs `WriteBlock` diagram | Show why HTTP input grouping and store/output grouping are different | `Store.md` |
| Buffer/queue pressure map | Place kernel buffers, Hyper body stream, HEC body limits, queues, and write buffers in order | `Stack.md` |
| Validation matrix | Tie each protocol condition to HTTP status, HEC code, metric, log/report fact, and test fixture | `HECpoc.md` Appendix 9 |


