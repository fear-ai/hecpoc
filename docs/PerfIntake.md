# PerfIntake — What To Move Or Distill From spank-rs/perf

Status: historical/supporting artifact under `docs/`. This file is not an active design authority; use it only for benchmark ideas, performance cautions, and migration reminders that have not yet been incorporated into current HECpoc specs.

Focus: plan. This note reviews `/Users/walter/Work/Spank/spank-rs/perf` as input to the standalone HEC PoC repository at `/Users/walter/Work/Spank/HECpoc`. It recommends what to move, what to distill, and what to leave behind. It does not copy the full contents of the perf documents, and it does not modify `spank-rs`.

Update this note when `HECpoc` gets its own requirements, validation, design, benchmark, or fixture files. Once those files exist, this note should become a migration checklist rather than a standing design document.

---

## 1. Summary Recommendation

Do not move `spank-rs/perf` wholesale into `HECpoc`. The perf directory is a mixed control layer: product orientation, feature matrix, capsule briefs, validation lab notes, documentation restructuring proposals, and a Rust performance harness. Moving it as-is would recreate the same history/status sprawl that `HECpoc` is meant to escape.

Do distill four things immediately:

1. HEC capsule requirements and acceptance tests from `Capsules.md §2`.
2. HEC-specific feature rows from `Features.csv` and requirement prose from `Features.md §8`.
3. HEC validation lab procedures from `Tools.md §2` through `§13`.
4. Sink, parser, and benchmark lessons from `SpankMax.md §2` and `§3.0`, plus selected source patterns from `perf/src`.

Keep `Three.md`, `Orient.md`, and `Redoc.md` mostly as cited background. They are valuable for context, but they are too broad for a focused HEC PoC repository.

---

## 2. File-Level Triage

The table below classifies each perf artifact by recommended action.

| Source | Action | Rationale | HECpoc destination |
|--------|--------|-----------|--------------------|
| `/Users/walter/Work/Spank/spank-rs/perf/Capsules.md` | Distill, not move | `§2` defines the lead HEC CI Fixture capsule; later capsule sections are useful context but not PoC scope | `Requirements.md` and `README.md` |
| `/Users/walter/Work/Spank/spank-rs/perf/Features.csv` | Filter and copy subset | The full matrix covers Sigma, SPL, embedded, small deploy, ops, and performance; HECpoc needs only the HEC PoC subset | `requirements/hec-poc.csv` |
| `/Users/walter/Work/Spank/spank-rs/perf/Features.md` | Distill requirement prose | `§8.1`, `§8.2`, `§8.6`, and `§8.7` explain rows relevant to HEC; the matrix method belongs as background | `Requirements.md` |
| `/Users/walter/Work/Spank/spank-rs/perf/Tools.md` | Split and distill heavily | It contains the most directly useful HEC validation material, but also broad corpora and source references | `Validation.md`, `configs/`, `requests/`, `results/README.md` |
| `/Users/walter/Work/Spank/spank-rs/perf/SpankMax.md` | Distill design principles only | It is a performance harness manual, not a HEC protocol spec; sink and benchmark ideas matter | `Design.md` and `Benchmark.md` |
| `/Users/walter/Work/Spank/spank-rs/perf/src/*` | Cherry-pick patterns later | Useful parser, normalization, tokenization, store, and fixture patterns exist; importing now would drag engine work ahead of HEC acceptance | later `bench/` or `crates/` only after PoC gates pass |
| `/Users/walter/Work/Spank/spank-rs/perf/Orient.md` | Cite and summarize | Good effort-level diagnosis and lead-capsule decision; too broad for direct move | short `README.md` background paragraph |
| `/Users/walter/Work/Spank/spank-rs/perf/Three.md` | Leave as background | It is a broad effort-level review with product landscape and original prompts; direct move would swamp the PoC | cite in `README.md` or `docs/background.md` only if needed |
| `/Users/walter/Work/Spank/spank-rs/perf/Redoc.md` | Leave in spank-rs | It is about restructuring `spank-rs` docs, not designing HECpoc | no direct destination |
| `/Users/walter/Work/Spank/spank-rs/perf/Cargo.toml` and `Cargo.lock` | Do not move now | They belong to `spankmax`; HECpoc should not inherit dependencies before design selects code | later benchmark crate if copied deliberately |
| `/Users/walter/Work/Spank/spank-rs/perf/target/` | Never move | Build artifact | none |

The important split is: product and validation material should become HECpoc control documents; performance harness material should remain a reference until a benchmark work package starts.

---

## 3. Immediate HECpoc Documents To Create

`HECpoc.md` now defines the starting process. The next documents should be fewer and sharper than the perf layer.

```text
/Users/walter/Work/Spank/HECpoc/
  README.md
  HECpoc.md
  Requirements.md
  Validation.md
  Design.md
  Benchmark.md
  PerfIntake.md
  requirements/
    hec-poc.csv
  configs/
    spank/
    vector/
    splunk/
  requests/
    curl-matrix.md
    bodies/
  results/
    README.md
```

Recommended ownership:

| HECpoc file | Content | Distilled from |
|-------------|---------|----------------|
| `README.md` | What this repo is, what it is not, how to run the first PoC validation | `Orient.md §5`, `Capsules.md §2`, `HECpoc.md §4` |
| `Requirements.md` | HEC PoC requirement subset, feature IDs, non-requirements, acceptance matrix | `Features.csv`, `Features.md §8`, `Capsules.md §2` |
| `requirements/hec-poc.csv` | Filtered requirement rows only | `Features.csv` |
| `Validation.md` | curl, Vector, Splunk Enterprise, tutorial logs, ledger fields, pass/fail classes | `Tools.md §2` through `§13` |
| `Design.md` | HEC boundary, sink trait, current-code disposition, route/parser/sink decisions | `HECpoc.md §7` through `§11`, `SpankMax.md §2` and `§3.0` |
| `Benchmark.md` | Null/capture/SQLite sink measurements, startup, ingest latency, corpus profiles | `SpankMax.md §4` through `§6`, `Tools.md §12.5` |
| `PerfIntake.md` | Migration checklist and anti-sprawl guard | this file |

Do not create a large `docs/` tree yet. If the workbench has too many documents before tests exist, it is already drifting.

---

## 4. Requirement Distillation

The filtered HEC PoC requirement table should start with only rows needed to accept, store, inspect, and validate HEC traffic.

Recommended required rows:

| ID | Reason to include |
|----|-------------------|
| `ING-HEC-JSON` | Core endpoint under test |
| `ING-HEC-AUTH` | Real HEC clients depend on token behavior |
| `ING-HEC-GZIP` | Common HEC client behavior; cheap to test |
| `ING-HEC-RAW` | Needed for basic raw endpoint compatibility, but can be second wave |
| `ING-BACKPRESS` | Prevents false success under overload |
| `EVT-RAW` | Ground truth for every stored event |
| `EVT-TIME` | Required for inspection and comparison to Splunk searches |
| `EVT-HOST` | HEC metadata and Splunk default-field expectation |
| `EVT-SOURCE` | HEC metadata and fixture assertions |
| `EVT-SOURCETYPE` | Parser/profile routing and Splunk-ish event identity |
| `EVT-INDEX` | Logical namespace and token allowed-index behavior |
| `SCH-TERM` | Minimum query/assertion surface over stored events |
| `SCH-TIME` | Minimum time-bounded inspection |
| `OBS-METRICS` | Required to explain success, rejection, and backpressure |
| `PKG-LOCALCFG` | Required for repeatable fixture startup and parallel tests |
| `PERF-STARTUP` | Fixture product fails if startup is slow |

Recommended deferred rows:

| ID | Reason to defer |
|----|-----------------|
| `ING-HEC-ACK` | Requires durable commit boundary and ack state; dangerous before sink semantics are firm |
| `PKG-PYTEST` | Product wrapper question should not dictate Rust PoC internals yet |
| `PAR-SYSLOG`, `PAR-AUTH`, `PAR-APACHE-ACC` | Useful for pressure tests after raw HEC event preservation passes |
| `SCH-FIELDS`, `SCH-WHERE`, `SCH-RAWVERIFY` | Needed soon, but should follow the first minimal inspection API |
| `OPS-DURABLE` | SQLite acceptance can be durable enough for PoC; production durability is separate |
| `PERF-INGEST`, `PERF-QUERY` | Benchmark once correctness and sink boundaries are fixed |

The first `requirements/hec-poc.csv` should not include Sigma, SPL tutorial, embedded, or small-deploy columns. Those belong to Spank-wide product planning, not the HEC PoC repo.

---

## 5. Validation Distillation

`Tools.md` should be mined aggressively because it is the strongest HECpoc candidate. The distilled validation document should keep exact runnable flows and delete broad research commentary.

Move or rewrite into `Validation.md`:

1. Tool roles: Splunk Enterprise as reference, Vector as real HEC client, curl as protocol probe, Universal Forwarder as S2S-only reference.
2. Local paths and ports: Splunk tutorial data, local log corpora, Spank bind, Splunk HEC bind, Vector config paths.
3. curl tests: health, good JSON event, bad token, malformed JSON, raw endpoint, gzip, channel/ACK preflight.
4. Vector tests: Vector to Splunk reference, Vector to Spank, input progression.
5. Splunk Enterprise reference searches: `_raw`, `_time`, `host`, `source`, `sourcetype`, counts.
6. Run classes: protocol conformance, shipper compatibility, event model, parser pressure, benchmark.
7. Ledger fields: command, config, input corpus, expected rows, accepted rows, rejected rows, outcome codes, sink path, metrics snapshot.

Do not move into HECpoc yet:

1. Exhaustive external corpora list.
2. Broad Sigma/parser pressure runs.
3. Universal Forwarder setup beyond the single note that it is not a HEC sender.
4. Historical source references unless directly used by a runnable validation step.

A good `Validation.md` should read like a lab manual, not a research survey.

---

## 6. Design Distillation

`SpankMax.md` and `perf/src` should influence design, but they should not become the HECpoc implementation by default.

Distill these design points:

1. Separate ingestion from search prep.
2. Keep a scalar correctness path before optimized paths.
3. Use bounded staging and explicit backpressure.
4. Prefer existing proven crates before unsafe/SIMD work.
5. Keep raw/capture/SQLite/null sinks behind a small trait.
6. Treat parser/profile IDs as compact internal concepts later, not as strings in hot loops.
7. Keep CPU-heavy parsing/tokenization separate from Tokio network handler work when that work becomes nontrivial.

Do not import these prematurely:

1. Full parser dispatch from `perf/src/parsers.rs`.
2. Normalized column batch layout from `perf/src/normalize.rs`.
3. Tokenizer/posting structures from `perf/src/tokenize.rs`.
4. SQLite benchmark schema from `perf/src/store.rs` as product schema.
5. `spankmax` CLI as a required HECpoc binary.

The first HECpoc design should define three seams: HEC parser, ingest queue, and sink/inspection. SpankMax concepts enter only when a benchmark needs them.

---

## 7. What To Leave Behind

Leave behind broad effort history unless it directly changes HEC PoC behavior.

`Three.md` should remain an effort-level review. It is useful for understanding why the effort shifted from Python to Rust and from broad Splunk clone ideas to product capsules. It should not be copied into HECpoc because it would make the new repo inherit the emotional and historical weight of the prior attempts.

`Orient.md` should be cited for the decision to lead with HEC CI Fixture and use the matrix as control surface. It should not be moved whole because it also covers Sigma, SPL, embedded, small deployment, documentation authority, and product-wide sequencing.

`Redoc.md` should stay in `spank-rs`. HECpoc can adopt its lesson, not its content: document classes and ownership matter, but the HEC PoC repo should start with a smaller document system.

`SpankMax.md` should stay with the performance harness until HECpoc has a benchmark crate or benchmark command. Copying it early would bias the PoC toward engine optimization before protocol correctness and validation evidence are stable.

---

## 8. Suggested Migration Order

The safest sequence is:

1. Create `Requirements.md` and `requirements/hec-poc.csv` from `Features.csv`, `Features.md §8`, and `Capsules.md §2`.
2. Create `Validation.md` from `Tools.md §2` through `§13`, keeping only runnable HEC procedures.
3. Create `Design.md` from `HECpoc.md §7` through `§11`, current `spank-hec` crosswalk, and selected `SpankMax.md` principles.
4. Create `README.md` last, after the above files can state what this repo actually does.
5. Create `Benchmark.md` only after the first correctness validation path passes.
6. Consider copying or reimplementing `spankmax` pieces only after benchmark goals name exact sink/parser profiles.

This order keeps HECpoc from becoming another broad design anthology.

---

## 9. Concrete Next Extraction

The next extraction should be small enough to review in one sitting:

1. Write `requirements/hec-poc.csv` with the selected IDs from `Features.csv`.
2. Write `Requirements.md` explaining each selected ID in HEC-specific terms.
3. Add a traceability table from requirement ID to first test name.
4. Leave all code unmoved.

This gives the new repository its first durable control artifact: a requirement subset that can govern code, validation, and later benchmarks.
