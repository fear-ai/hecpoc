# Store — Event Pipeline, Queue Topology, Evidence Storage, And Retirement

Status: design reference and requirements input.

Scope: application-level processing after HEC/body framing has produced bounded decoded input and before stored evidence is retired. This document owns queue topology, batch boundaries, parser handoff, token/index construction, sink states, durability semantics, store layout alternatives, profile-specific tuning, and validation for queue/store behavior.

This is not a database-choice note. The first durable question is what state the receiver can honestly claim for an accepted request. Database selection is a later implementation choice under that state model.

---

## 1. Problem Statement

HECpoc currently proves that an HTTP handler can accept bounded HEC requests and either drop or capture events. That is not yet an ingest system. An ingest system must answer these questions without ambiguity:

1. What work happens on the request path?
2. Where does work leave the request path?
3. What is queued, in what unit, with which limit?
4. What does HTTP success mean for the selected sink mode?
5. What can be replayed after a crash?
6. What is searchable immediately, later, or never?
7. When can a block of information be retired?

Without this structure, benchmark results are not comparable. A drop sink, capture file, buffered writer, `fsync` per request, SQLite transaction, and token-indexing path can all accept the same input while measuring different systems.

Recommendation: define store and pipeline states before optimizing storage. Start with append-only evidence, then add queue isolation, then add durable commit, then add search-prep workers. Do not let parser/index ambitions rewrite the HEC request handler.

---

## 2. Ingest-To-Retirement Sequence

Use this sequence instead of the earlier unclear phrase “byte-to-retirement.” The sequence begins after transport/body handling has produced decoded bytes or parsed HEC envelopes and ends when stored blocks are deleted or compacted.

```text
bounded decoded input
  -> event boundary selection
  -> parser dispatch or raw preservation
  -> EventBatch construction
  -> admission to queue or synchronous fixture sink
  -> sink worker batch receipt
  -> evidence write
  -> optional flush
  -> optional durable commit
  -> optional parser/token/index workers
  -> sealed information block
  -> query/inspection access
  -> compaction or retirement
```

State vocabulary:

| State | Meaning | HTTP/ACK Implication |
|---|---|---|
| `formed` | request bytes became one or more event candidates | not enough for success if sink is required |
| `accepted` | event batch passed protocol and parser validation for the selected mode | may return success only in explicit in-memory/drop benchmark mode |
| `queued` | batch entered bounded store/sink handoff | acceptable benchmark ACK boundary only if declared |
| `written` | sink write call returned | not necessarily durable |
| `flushed` | userspace writer flushed to kernel/page cache | not durable against power loss |
| `durable` | `fsync`, DB commit, or equivalent durable boundary completed | first honest production ACK boundary |
| `indexed` | search-prep/token/field indexes are built for the batch/block | query acceleration available |
| `sealed` | block no longer accepts writes and has stable metadata | safe for compaction/retention decisions |
| `retired` | block was deleted, compacted away, or archived | no longer in active local search scope |

Recommendation: HTTP success for the initial capture mode should mean `written` or `flushed` only if the handler waits for that state. If the handler returns after `queued`, the response and benchmark profile must say `queued`, not captured or durable.

---

## 3. Pipeline Boundaries

### 3.1 Request Path

Allowed on the request path for the first implementation:

- auth and HEC response classification;
- bounded body read and optional gzip decode;
- raw endpoint line boundary detection for bounded bodies;
- HEC JSON envelope parsing for bounded bodies;
- shallow event metadata extraction;
- `EventBatch` construction;
- bounded queue admission or synchronous fixture write.

Not allowed on the request path once queue mode exists:

- long file writes;
- database transactions;
- full sourcetype parsing for large bodies;
- tokenization/index construction;
- compression of store blocks;
- unbounded retries;
- waits on a full queue without an explicit enqueue timeout policy.

### 3.2 Sink Worker Path

The sink worker receives `EventBatch` units and owns store-specific buffering and commit state. It may write raw/capture evidence, group batches into transactions, flush buffered writers, or emit work to search-prep workers.

The sink worker must report:

- batches received;
- events received;
- bytes received;
- queue wait time;
- write duration;
- flush duration;
- durable commit duration if enabled;
- failures by reason;
- last successful commit state.

### 3.3 Search-Prep Worker Path

Search-prep workers operate on evidence that can be replayed. They must not be the only place where accepted input exists.

Search-prep work includes:

- sourcetype-specific deeper parsing;
- normalization to canonical fields;
- alias view construction;
- tokenization;
- position or proximity indexes;
- field-aware indexes;
- Sigma-oriented literal/field accelerators;
- sealed-block metadata.

Recommendation: search-prep workers consume sealed or replayable batches. Do not couple HTTP success to token/index completion until the product bundle explicitly requires query-ready success.

---

## 4. Queue Topology Alternatives

No queue topology is final. The decision should be driven by fairness, cache locality, sink layout, and failure containment.

| Topology | Advantages | Risks | Best Use |
|---|---|---|---|
| single global queue | simplest, easiest stats, easiest backpressure | one hot source can dominate; no locality | first bounded handoff test |
| per-peer queue | limits noisy clients and brute-force sources | many small queues, peer churn | HEC exposed to multiple hosts |
| per-token/source queue | aligns with HEC token/source policies | token cardinality can grow | compatibility lab and multi-tenant fixture |
| per-sourcetype queue | keeps parser-specific work localized | sourcetype inference can be wrong or late | parser-heavy ingestion |
| per-CPU queue | cache locality, less contention | harder ordering and rebalancing | high-throughput parser/index workers |
| per-store-partition queue | aligns with DB/chunk writes | prematurely couples ingest to store layout | mature durable store |
| hybrid admission + worker queues | separate fairness from CPU/store layout | more counters and tuning | production candidate |

Recommended sequence:

1. One global bounded queue to prove backpressure and queue-full responses.
2. Per-token/source accounting without separate queues.
3. Per-sourcetype or per-worker queues only after parser/index benchmarks show contention or cache-locality benefit.
4. Store-partitioned queues only after block/chunk layout exists.

Policy options per queue:

| Policy | Meaning | Use Case |
|---|---|---|
| reject newest | incoming batch fails immediately when full | HEC correctness and retryable overload |
| wait bounded | enqueue waits up to configured timeout | short sink jitter without lying to clients |
| drop oldest | sacrifice stale queued data for latest | telemetry mode, not default HEC correctness |
| spill to disk | overflow memory to disk | production durability candidate, higher complexity |
| priority lanes | health/control/auth separate from bulk ingest | keep readiness responsive under load |

Recommendation: default HEC mode should reject newest with HEC server-busy semantics when queue is full. Head-drop/drop-oldest is a telemetry policy, not a correctness-first HEC policy.

---

## 5. Batch And Granularity Choices

Pipeline tuning begins with the unit of work.

| Unit | Description | Strength | Risk |
|---|---|---|---|
| request batch | all events from one HEC request | preserves HEC response/ACK scope | large uneven batches |
| fixed event batch | N events per batch | predictable worker cost | splits request identity |
| byte-sized batch | target decoded/raw byte size | cache and IO friendly | event count varies widely |
| time-slice batch | collect until duration expires | latency bound | less deterministic benchmark results |
| store-block batch | collect until block target size | efficient sealing/indexing | too late for request-level feedback |

Recommendation: keep `EventBatch` as request-scoped at ingress, then allow sink workers to coalesce into store-block batches. Store-block batching should not erase request ID, channel, token, source, or commit-state provenance.

Batch metadata requirements:

- request ID;
- endpoint kind;
- token/source class;
- source, sourcetype, host, index when known;
- raw byte count;
- decoded byte count;
- event count;
- parser status counts;
- queue admission timestamp;
- sink receipt timestamp;
- commit state timestamp.

---

## 6. Store Layout Alternatives

### 6.1 Capture JSONL

Append one event per line as JSON. This is the simplest inspection format.

Strengths:

- readable;
- easy to diff;
- easy test assertions;
- works with `jq` and ordinary tooling.

Risks:

- escaping overhead;
- weak corruption recovery unless line boundaries remain intact;
- poor random access for large files;
- JSON serialization may hide raw-byte distinctions.

Recommendation: use capture JSONL for fixture and compatibility lab mode, not as the final performance store.

### 6.2 Length-Delimited Raw Chunks

Store length-prefixed raw events or batches with a checksum per record or block.

Strengths:

- byte preservation;
- corruption localization;
- replay-friendly;
- fewer escaping costs.

Risks:

- needs inspection tooling;
- field visibility requires sidecar metadata or later parsing;
- block format must be specified carefully.

Recommendation: likely second storage format after JSONL once replay and durability matter.

### 6.3 Column/Segment Store

Store hot fields and token/index structures in separate segment files by sealed block.

Strengths:

- search-friendly;
- skip/selectivity metadata;
- can optimize for Sigma/SPL cases;
- allows per-field compression and encoding.

Risks:

- more files and metadata;
- commit consistency problem across segments;
- schema/alias evolution complexity.

Recommendation: do not start here. Build only after raw/capture replay can rebuild segments.

### 6.4 Embedded Database

SQLite, DuckDB, or similar embedded engines provide query and transaction machinery.

Strengths:

- quick inspection and indexing;
- transactional semantics;
- less custom store code.

Risks:

- DB transaction cost can dominate ingest;
- schema changes can drag the project back into migration machinery;
- query model may fight Sigma/SPL-specific token indexes.

Recommendation: useful for inspection and early durable query, but keep raw/capture evidence as replay source.

### 6.5 Partitioning By Host, File, Or Log Type

Per-host, per-file, or per-log-type databases look attractive because ownership is obvious and a failed input source appears isolated. They are weak default ingest partitions because they multiply active writers, file handles, schemas, compaction surfaces, recovery paths, and query fanout before the workload has proven stable.

The worst-case active-target count is roughly:

```text
active targets = hosts * log types * files * active time partitions
```

That Cartesian product is an ingest penalty even when later queries usually filter by only one or two dimensions.

Recommended initial model:

- append into a small number of hot buckets selected by time/size and possibly benchmark profile;
- store host, source, sourcetype, index, file path, and parser family as metadata, not as database identity;
- seal buckets into immutable information blocks;
- build per-field, per-token, or per-format sidecar structures against sealed blocks;
- coalesce or repartition sealed data later only when measurements show a search or retention benefit.

A dedicated worker for one huge file, one dominant sourcetype, or one high-cost parser may be justified. That is routing and scheduling, not a reason to create a separate database family during ingest.

---

## 7. Tokenization And Index Construction

Token/index work is search-preparation, not HEC request acceptance.

### 7.1 Processing Stages

Pipeline stages:

```text
raw or parsed event
  -> breaker/tokenizer
  -> normalization/folding policy
  -> field-aware token assignment
  -> optional position/proximity records
  -> per-block term dictionary
  -> per-term postings or field segment
  -> sealed-block skip metadata
```

Optimization choices:

| Technique | Candidate Use | Default? |
|---|---|---:|
| `memchr` | separators and byte sentinels | yes |
| `aho-corasick` | many literals, Sigma keywords, attack strings | yes for search-prep |
| Rust `regex` | untrusted extraction and filters | yes with limits |
| SIMD scanners | high-volume delimiters/classes after scalar proof | no, benchmark-gated |
| tuned assembly | only for stable hot loops with large win | no |
| per-format tokenizer | Apache URI, syslog process/message, audit key/value | later |
| field-aware index | Sigma/SPL field restrictions | later |
| proximity/position index | phrase/proximity search | later, feature-gated |

Recommendation: implement scalar correctness first, then add optimized variants behind agreement tests. A faster tokenizer that changes tokens is a bug, not an optimization.

### 7.2 Prior Prototype Signals

Prior prototype material is useful as evidence, not as code to lift blindly:

| Source | Useful Signal | Store/Pipeline Interpretation |
|---|---|---|
| `/Users/walter/Work/Spank/spank-rs/perf/src/parsers.rs` | scalar and `memchr` parser direction | candidate delimiter and parser benchmark implementation after raw correctness is fixed |
| `/Users/walter/Work/Spank/spank-rs/perf/src/normalize.rs` | field normalization and batch-oriented values | future canonical field/value batch representation |
| `/Users/walter/Work/Spank/spank-rs/perf/src/tokenize.rs` | search-prep token construction | replayable token worker, not request-path logic |
| `/Users/walter/Work/Spank/spank-rs/perf/src/store.rs` | null/raw/SQLite benchmark sinks | benchmark-profile store variants, not production schema by default |
| `spank-hec/src/receiver.rs` | bounded receiver and queue/backpressure ordering | validates need for explicit queue state and pressure counters |
| `spank-hec/src/processor.rs` | event/null/time/parser test ideas | fixture coverage source, with current limits and HEC outcomes restated before reuse |

---

## 8. Production Profiles And Benchmark Profiles

Profiles prevent benchmark decisions from leaking into product claims.

| Profile | Success Boundary | Queue Policy | Store Policy | Intended Claim |
|---|---|---|---|---|
| `fixture-capture` | written or flushed capture file | reject newest on full | JSONL/capture | testable local HEC fixture |
| `drop-benchmark` | accepted or queued | configurable | drop sink | parser/HTTP upper bound only |
| `queue-benchmark` | queued | reject newest or bounded wait | no durable claim | handoff/backpressure capacity |
| `durable-capture` | durable raw/capture block | reject newest or spill | fsync/DB commit | production-like durability baseline |
| `indexed-store` | durable + indexed | queue plus search-prep workers | sealed blocks + indexes | query-ready ingest |

Every benchmark result must name its profile. A `drop-benchmark` number cannot be cited as ingest capacity for a durable store.

---

## 9. Validation Owned Here

Queue/store validation is part of the store design, not an afterthought.

| Validation Group | Required Cases |
|---|---|
| queue admission | empty queue, full queue, simultaneous producers, enqueue timeout, closed receiver |
| queue fairness | hot peer/source does not starve control/health path; per-source counters visible |
| sink states | accepted, queued, written, flushed, durable, failed-after-accept |
| write failures | permission denied, ENOSPC, short write, interrupted write, path removed |
| durability | crash before flush, crash after flush, crash after fsync/commit |
| replay | rebuild parser/index output from raw/capture evidence |
| batching | request-sized, fixed event count, byte-sized, and store-block batches produce equivalent event sets |
| token/index | scalar and optimized tokenizers agree on token stream and field assignment |
| retention | sealed block deletion does not remove blocks still referenced by active metadata |

Metrics/counters needed for validation:

- queue depth current/max;
- queue rejected total by reason;
- enqueue wait histogram;
- sink write duration histogram;
- flush duration histogram;
- durable commit duration histogram;
- batches by commit state;
- events by commit state;
- bytes by commit state;
- replay failures;
- sealed blocks created/retired.

---

## 10. Tuning Backward Through The Pipeline

Store and search requirements should tune earlier stages deliberately.

Method:

1. Identify query and inspection requirements.
2. Determine which fields/tokens must exist at search time.
3. Decide whether those fields/tokens are materialized during ingest or built asynchronously.
4. Choose batch size and queue topology to match the chosen materialization stage.
5. Choose store block size and sealing rules.
6. Tune parser and tokenizer parallelism.
7. Tune network/body limits only after store pressure is visible.

Examples:

- If Sigma rules mostly need URI literals and status codes, Apache/Nginx search-prep should materialize `uri`, `status`, `method`, `src_ip`, and suspicious literal tokens before broad free-text indexing.
- If auth investigations need `user`, `src_ip`, and sshd action, auth.log parsing should materialize those fields before tokenizing the entire message body.
- If capture mode is only for CI assertions, JSONL readability beats custom binary throughput.
- If durable production mode is required, `fsync` or DB commit cost sets the true upper ingest rate, not Axum request/sec.

---

## 11. Open Questions

| Area | Question | Blocking Condition |
|---|---|---|
| queue topology | global queue, per-source accounting, or per-source queues first? | before implementing queue worker beyond one global queue |
| queue-full behavior | reject newest, bounded wait, spill, or drop oldest per mode? | before advertising backpressure behavior |
| capture format | JSONL, length-delimited JSON, raw chunks, or dual raw+metadata? | before relying on capture for replay |
| success boundary | does HTTP success mean queued, written, flushed, or durable for each mode? | before ACK or production claims |
| store block size | event count, byte size, or time window? | before sealed blocks or index segments |
| index timing | ingest-time, asynchronous replay, or query-time? | before Sigma/search acceleration |
| raw preservation | byte-exact raw bytes or decoded text only in first durable format? | before malicious/non-UTF validation claims |
| optimized tokenizers | scalar only, SIMD-gated, or per-format optimized variants? | before performance claims beyond correctness path |
| retention | time-based, size-based, source/index-based, or capability-bundle-specific? | before any block retirement implementation |
