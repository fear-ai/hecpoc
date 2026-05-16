# Store — Event Pipeline, Queue Topology, Evidence Storage, And Retirement

Scope: application-level processing after HEC protocol handling has produced `HecEvents` and before stored evidence is retired. This document owns event disposition, queue topology, `ParseBatch` policy, `WriteBlock` construction, commit states, durability semantics, store layout alternatives, profile-specific tuning, and validation for queue/store behavior.

Mandate: define what happens to accepted HEC events after protocol validation, including queue boundaries, write aggregation, commit-state truthfulness, durable evidence, replay, and retirement. Store does not own HEC request parsing, HTTP mechanics, format grammar details, or generic reporting/configuration machinery.

This is not a database-choice note. The first durable question is what state the receiver can honestly claim for an accepted request. Database selection is a later implementation choice under that state model.

---

## 1. Problem Statement

HECpoc currently proves that an HTTP handler can accept bounded HEC requests and either drop or capture events. That is not yet an ingest system. An ingest system must answer these questions without ambiguity:

1. Which validated `HecEvents` can the HEC request path produce?
2. Which disposition occurs: reject, enqueue, write, drop, or forward?
3. What is queued, in what unit, with which limit?
4. Which commit state does HTTP success or ACK claim?
5. What can be replayed after a crash?
6. What is searchable immediately, later, or never?
7. When can stored evidence be compacted, archived, or retired?

Without this structure, benchmark results are not comparable. A drop sink, capture file, buffered writer, `fsync` per request, SQLite transaction, and token-indexing path can all accept the same input while measuring different systems.

Recommendation: define store and pipeline states before optimizing storage. Start with append-only evidence, then add queue isolation, then add durable commit, then add search-preparation execution. Do not let parser/index ambitions rewrite the HEC request handler.

---

## 2. HecEvents-To-Retirement Sequence

The sequence begins after HEC protocol handling has produced accepted `HecEvents` and ends when stored evidence is deleted, compacted, or archived.

```text
HecEvents
  -> disposition: reject, enqueue, write, drop, or forward
  -> optional bounded queue
  -> write path receipt
  -> WriteBlock construction when buffering or durable output is enabled
  -> evidence write
  -> optional flush
  -> optional durable commit
  -> optional format interpretation
  -> optional ParseBatch construction
  -> optional search preparation
  -> query/inspection access
  -> compaction, archive, or retirement
```

State vocabulary:

| State | Meaning | HTTP/ACK Implication |
|---|---|---|
| `accepted` | HEC validation produced one or more valid `HecEvents` | may return success only in explicit in-memory/drop benchmark mode |
| `queued` | `HecEvents` entered a bounded queue | acceptable benchmark ACK boundary only if declared |
| `written` | write call returned for the selected sink/store | not necessarily durable |
| `flushed` | userspace writer flushed to kernel/page cache | not durable against power loss |
| `durable` | `fsync`, DB commit, or equivalent durable boundary completed | first honest production ACK boundary |
| `search_ready` | search-prep/token/field structures are built for the relevant evidence | query acceleration available |
| `retired` | evidence was deleted, compacted away, or archived | no longer in active local search scope |

Recommendation: HTTP success for the initial capture mode should mean `written` or `flushed` only if the handler waits for that state. If the handler returns after `queued`, the response and benchmark profile must say `queued`, not written, flushed, or durable.

---

## 3. Pipeline Boundaries

### 3.1 HEC Request Path

Allowed on the HEC request path for the first implementation:

- auth and HEC response classification;
- bounded body read and optional gzip decode;
- raw endpoint event formation for bounded bodies, with split-line default and optional raw-byte evidence storage;
- HEC JSON envelope parsing for bounded bodies;
- shallow event metadata extraction;
- `HecEvents` formation;
- bounded queue insertion or synchronous fixture write.

Not allowed on the request path once queue mode exists:

- long file writes;
- database transactions;
- full sourcetype parsing for large bodies;
- tokenization/index construction;
- compression of store blocks;
- unbounded retries;
- waits on a full queue without an explicit enqueue timeout policy.

### 3.2 Write Path

The write path receives `HecEvents` or `WriteBlock` units and owns store-specific buffering and commit state. It may write raw/capture evidence, group events into transactions, flush buffered writers, or emit replayable evidence to search preparation.

The write path must report:

- `HecEvents` or `WriteBlock` units received;
- events received;
- bytes received;
- queue wait time;
- write duration;
- flush duration;
- durable commit duration if enabled;
- failures by reason;
- last successful commit state.

### 3.3 Search Preparation Path

Search preparation operates on evidence that can be replayed. It must not be the only place where accepted input exists.

Search-prep work includes:

- sourcetype-specific deeper parsing;
- normalization to canonical fields;
- alias view construction;
- tokenization;
- position or proximity indexes;
- field-aware indexes;
- Sigma-oriented literal/field accelerators;
- store-block metadata.

Recommendation: search preparation consumes replayable evidence and may use `ParseBatch` only when measured parser/cache behavior justifies grouping. Do not couple HTTP success to token/index completion until the product bundle explicitly requires query-ready success.

---

## 4. Queue Topology Alternatives

No queue topology is final. The decision should be driven by fairness, cache locality, sink layout, and failure containment.

| Topology | Advantages | Risks | Best Use |
|---|---|---|---|
| single global queue | simplest, easiest stats, easiest backpressure | one hot source can dominate; no locality | first bounded queue test |
| per-peer queue | limits noisy clients and brute-force sources | many small queues, peer churn | HEC exposed to multiple hosts |
| per-token/source queue | aligns with HEC token/source policies | token cardinality can grow | compatibility lab and multi-tenant fixture |
| per-sourcetype queue | keeps parser-specific work localized | sourcetype inference can be wrong or late | parser-heavy ingestion |
| per-CPU queue | cache locality, less contention | harder ordering and rebalancing | high-throughput parser/index execution |
| per-store-partition queue | aligns with DB/chunk writes | prematurely couples ingest to store layout | mature durable store |
| hybrid front queue + function queues | separate fairness from CPU/store layout | more counters and tuning | production candidate |

Recommended sequence:

1. One global bounded queue to prove backpressure and queue-full responses.
2. Per-token/source accounting without separate queues.
3. Per-sourcetype or per-function queues only after parser/index benchmarks show contention or cache-locality benefit.
4. Store-partitioned queues only after block/chunk layout exists.

Policy options per queue:

| Policy | Meaning | Use Case |
|---|---|---|
| reject newest | incoming unit fails immediately when full | HEC correctness and retryable overload |
| wait bounded | enqueue waits up to configured timeout | short sink jitter without lying to clients |
| drop oldest | sacrifice stale queued data for latest | telemetry mode, not default HEC correctness |
| spill to disk | overflow memory to disk | production durability candidate, higher complexity |
| priority lanes | health/control/auth separate from bulk ingest | keep readiness responsive under load |

Recommendation: default HEC mode should reject newest with HEC server-busy semantics when queue is full. Head-drop/drop-oldest is a telemetry policy, not a correctness-first HEC policy.

---

## 5. HecEvents, ParseBatch, And WriteBlock Granularity

Pipeline tuning begins with the unit of work.

| Unit | Description | Strength | Risk |
|---|---|---|---|
| `HecEvents` | all accepted events from one HEC request | preserves HEC response/ACK provenance | large uneven groups |
| `ParseBatch` by event count | N events selected for format interpretation | predictable parser cost | must preserve request provenance |
| byte-sized `ParseBatch` | target decoded/raw byte size | cache and CPU friendly | event count varies widely |
| time-window `ParseBatch` | collect until duration expires | latency bound | less deterministic benchmark results |
| `WriteBlock` | collect until store target size or flush policy fires | efficient output buffering and later indexing | too late for request-level feedback |

Recommendation: keep HEC request provenance at ingress, then allow write and search-preparation paths to split or coalesce into `WriteBlock` and `ParseBatch` units. Store/output aggregation must not erase request ID, channel, token, source, or commit-state provenance.

Provenance fields required on `HecEvents`, `ParseBatch`, or `WriteBlock` as applicable:

- request ID;
- endpoint kind;
- token/source class;
- source, sourcetype, host, index when known;
- raw byte count;
- decoded byte count;
- event count;
- parser status counts;
- queue insertion timestamp;
- write path receipt timestamp;
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

Store length-prefixed raw events or `WriteBlock` units with a checksum per record or block.

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

Store hot fields and token/index structures in separate segment files by `WriteBlock` or later store segment.

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
- close buckets into immutable information blocks after store block layout is defined;
- build per-field, per-token, or per-format sidecar structures against store blocks;
- coalesce or repartition immutable data later only when measurements show a search or retention benefit.

A dedicated execution path for one huge file, one dominant sourcetype, or one high-cost parser may be justified. That is routing and scheduling, not a reason to create a separate database family during ingest.

### 6.6 Backend Abstraction Must Not Hide Performance Truth

A common store interface is useful only if it preserves the operational differences that matter for correctness and performance. File append, buffered JSONL capture, length-delimited raw chunks, SQLite transactions, DuckDB batches, segment files, and a null/drop sink do not have the same write granularity, flush behavior, commit cost, failure modes, or readback capability.

The interface must expose capabilities and policies rather than pretending every backend is interchangeable.

Backend capability metadata should include:

- accepted input units: `HecEvents`, `RequestRaw`/`RawEvents` where byte preservation is needed, owned event objects, or `WriteBlock`;
- preferred write granularity: event count, byte size, elapsed time, or explicit flush boundary;
- commit states supported: written, flushed, durable, indexed;
- byte preservation: raw bytes, decoded text, dual raw+decoded, or structured-only;
- backpressure behavior: immediate reject, bounded wait, spill, drop, or internal buffering;
- replay support: can rebuild parsed fields, token indexes, and search-prep sidecars;
- ordering guarantee: per request, per source, per token/index, or only per store partition;
- observability fields: queue time, write time, flush time, commit time, and failure reason.

Do not hide backend cost behind a single `store.write(events)` claim without naming what that call proves. A null sink can measure HEC protocol and parsing overhead. A buffered file sink can measure append throughput. A durable store measures commit throughput. Those numbers are not substitutes for one another.

Recommended first interface shape:

- keep current fixture capture simple and explicit;
- add capability metadata before adding multiple production backends;
- choose `WriteBlock` construction from backend capability and benchmark profile;
- keep commit state in the caller-visible result, not buried inside backend logging;
- avoid early trait-object generality until two real backends need the same call shape.

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
  -> store-block skip metadata
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

### 7.2 Reuse Gate

Reuse existing parser, tokenizer, normalization, or storage code only after restating the current requirement, naming the target module, proving naming compatibility, and adding validation cases. Prior code is evidence, not an implementation contract.

---

## 8. Production Profiles And Benchmark Profiles

Profiles prevent benchmark decisions from leaking into product claims.

| Profile | Success Boundary | Queue Policy | Store Policy | Intended Claim |
|---|---|---|---|---|
| `fixture-capture` | written or flushed capture file | reject newest on full | JSONL/capture | testable local HEC fixture |
| `drop-benchmark` | accepted or queued | configurable | drop sink | parser/HTTP upper bound only |
| `queue-benchmark` | queued | reject newest or bounded wait | no durable claim | queue/backpressure capacity |
| `durable-capture` | durable raw/capture block | reject newest or spill | fsync/DB commit | production-like durability baseline |
| `indexed-store` | durable + search_ready | queue plus search preparation | store blocks + indexes | query-ready ingest |

Every benchmark result must name its profile. A `drop-benchmark` number cannot be cited as ingest capacity for a durable store.

---

## 9. Validation Owned Here

Queue/store validation is part of the store design, not an afterthought.

| Validation Group | Required Cases |
|---|---|
| queue insertion | empty queue, full queue, simultaneous producers, enqueue timeout, closed receiver |
| queue fairness | hot peer/source does not starve control/health path; per-source counters visible |
| sink states | accepted, queued, written, flushed, durable, failed-after-accept |
| write failures | permission denied, ENOSPC, short write, interrupted write, path removed |
| durability | crash before flush, crash after flush, crash after fsync/commit |
| replay | rebuild parser/index output from raw/capture evidence |
| grouping | request-sized `HecEvents`, fixed-count `ParseBatch`, byte-sized `ParseBatch`, and `WriteBlock` grouping produce equivalent event sets |
| token/index | scalar and optimized tokenizers agree on token stream and field assignment |
| retention | store block retirement does not remove evidence still referenced by active metadata |

Metrics/counters needed for validation:

- queue depth current/max;
- queue rejected total by reason;
- enqueue wait histogram;
- sink write duration histogram;
- flush duration histogram;
- durable commit duration histogram;
- HEC request groups or write units by commit state;
- events by commit state;
- bytes by commit state;
- replay failures;
- store blocks created/retired.

---

## 10. Tuning Backward Through The Pipeline

Store and search requirements should tune earlier stages deliberately.

Method:

1. Identify query and inspection requirements.
2. Determine which fields/tokens must exist at search time.
3. Decide whether those fields/tokens are materialized during ingest or built asynchronously.
4. Choose `ParseBatch`, `WriteBlock`, and queue topology to match the chosen materialization stage.
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
| queue topology | global queue, per-source accounting, or per-source queues first? | before implementing consumers beyond one global queue |
| queue-full behavior | reject newest, bounded wait, spill, or drop oldest per mode? | before advertising backpressure behavior |
| capture format | JSONL, length-delimited JSON, raw chunks, or dual raw+metadata? | before relying on capture for replay |
| success boundary | does HTTP success mean queued, written, flushed, or durable for each mode? | before ACK or production claims |
| store block size | event count, byte size, or time window? | before store blocks or index segments |
| index timing | ingest-time, asynchronous replay, or query-time? | before Sigma/search acceleration |
| raw preservation | byte-exact raw bytes or decoded text only in first durable format? | before malicious/non-UTF validation claims |
| optimized tokenizers | scalar only, SIMD-gated, or per-format optimized variants? | before performance claims beyond correctness path |
| retention | time-based, size-based, source/index-based, or capability-bundle-specific? | before any block retirement implementation |
