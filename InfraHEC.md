# InfraHEC — Infrastructure Spine

This document defines the reusable implementation services that feature modules depend on: configuration, startup, shutdown, error classification, reporting, logging, metrics, validation ledgers, benchmark ledgers, security defaults, and operational packaging.

Mandate: own the common machinery and conventions, not the product protocol, not log-format grammar, not network-stack mechanics, and not storage policy. Subject documents define their own parameters and behavior; this document defines how those parameters are loaded, validated, reported, redacted, measured, and operated.

The infrastructure rule is deliberately narrow:

```text
feature modules own meaning;
infrastructure owns repeatable mechanism.
```

## 1. Boundary And Inclusion Rules

Infrastructure belongs here only when it is shared by multiple modules or required before a module can safely run.

| Topic | Belongs Here | Does Not Belong Here |
|---|---|---|
| Configuration | source precedence, typed loading, validation API, redaction, effective-config evidence | domain-specific defaults, token/index rules, parser choices, queue capacities as product decisions |
| Errors | typed error categories, conversion boundaries, public/private text separation | protocol code tables, wire response matrices, parser-specific status catalogs |
| Reporting/logging | call-site contract, field typing, redaction, output routing, component filters | semantic definition of every domain event or counter |
| Metrics | naming conventions, registry/export approach, counter/histogram API expectations | every feature counter and every label value |
| Lifecycle | startup order, signal handling, graceful shutdown, panic policy | per-subsystem state machines unless they affect process lifecycle |
| Runtime policy | async/blocking rules, thread configuration surface, benchmark evidence requirements | low-level socket mechanics and HTTP parser behavior |
| Validation | evidence ledger format, compatibility-run recording, regression structure | protocol-specific truth tables or format-specific fixtures |
| Security | secret handling, redaction, hostile-input posture, safe failure principles | exact authentication scheme behavior or parser-specific rejection rules |

Borderline cases must name the owning subject:

- A stack byte limit is loaded and validated by infrastructure, but the network/runtime document owns where it fires.
- A parser timeout is loaded and recorded by infrastructure, but the format document owns why the timeout exists.
- A queue capacity is loaded and observed by infrastructure, but the store document owns queue topology and commit semantics.
- A public response is rendered through shared mechanisms, but the protocol document owns its status, code, and text.

## 2. Adopted Source Patterns

The older Spank documents are retained as sources only where they contribute a specific reusable pattern. A historical document reference is not a design authority by itself.

| Source | Material Carried Forward | Why It Remains Useful |
|---|---|---|
| `/Users/walter/Work/Spank/infra/Infrastructure.md` | layered view of infrastructure from process/runtime through operations; scale-regime distinction between local, lab, and production use | prevents a one-off receiver from hiding lifecycle, validation, and operations concerns until too late |
| `/Users/walter/Work/Spank/spank-py/Infra.md` | section cadence: problem, requirements, architecture, decision, call-site form, validation | useful as a writing pattern for infrastructure services; not a source of current implementation choices |
| `/Users/walter/Work/Spank/spank-rs/research/Infrust.md` | Rust-specific candidate libraries and concerns: `clap`, `figment`, `serde`, `tracing`, typed errors, Tokio lifecycle, test/benchmark tooling | converts previous Rust research into a bounded library-selection checklist |

Use of these documents is intentionally extractive: copy the rule, justification, or pattern that survives review; do not copy history, task status, or abandoned architecture.

## 3. Infrastructure Services

The implementation should keep these services visible as explicit modules or submodules. They do not need to become separate crates until they are complete and reusable.

| Service | Responsibility | Primary API Shape |
|---|---|---|
| Config | load defaults, file, CLI, env; validate; expose typed settings; render redacted effective config | `RuntimeConfig::load()` and typed sub-structs |
| Error | classify startup, config, request, decode, sink, runtime, and internal failures | local enums with `thiserror`; `anyhow` only at binary boundary |
| Public text | centralize user/client-visible strings and safe terminal output wording | constants or small render functions |
| Reporting | accept structured facts with component/phase/step/severity and typed fields | `reporter.record(...)` or narrowed call-site helpers |
| Logging | route reporting records to `tracing` and optional console output | `tracing-subscriber` with `EnvFilter` and JSON/plain formats |
| Metrics | update counters/histograms/gauges by explicit call-site intent | in-process counters first; Prometheus-compatible backend later |
| Lifecycle | startup sequence, shutdown signal, graceful drain, panic policy | `run(config).await` plus signal/drain routines |
| Validation ledger | record command, config, fixture, response, stats, logs, and environment | per-run directory with machine-readable summary |
| Benchmark ledger | record workload, payload, system state, throughput, latency, CPU/memory/fd/thread stats | JSON/TSV plus raw tool output |
| Security | secret redaction, hostile-input defaults, safe error text, resource-bound policy | redaction helpers and policy structs |

Infrastructure APIs should be boring and inspectable. Avoid generic frameworks that require domain modules to smuggle meaning through stringly maps.

## 4. Configuration

Configuration is a production subsystem, not incidental argument parsing.

### 4.1 Source Precedence

Use one deterministic precedence chain:

```text
compiled defaults < TOML config file < command line < environment
```

Recommended library stack:

- `serde` for typed config structs;
- `figment` for layered provider merging;
- `clap` for CLI parsing and help text;
- TOML for editable configuration files.

The point is not that these crates are magical. The point is to avoid hand-coded merge logic, duplicated defaults, and undocumented environment behavior.

### 4.2 Naming Rules

Every configurable parameter needs four stable names:

| Context | Form | Example Pattern |
|---|---|---|
| Rust field | `snake_case` | `max_http_body_bytes` |
| TOML key | dotted table/key | `limits.max_http_body_bytes` |
| CLI flag | kebab case | `--max-http-body-bytes` |
| Environment | prefix + uppercase | `APP_LIMITS_MAX_HTTP_BODY_BYTES` |

The examples are illustrative. Owning subject documents define the actual keys.

### 4.3 Validation Rules

Every parameter definition must state:

- type and unit;
- compiled default, if any;
- accepted range or set;
- invalid-value behavior;
- whether the value is safe to print;
- whether changing the value affects compatibility, safety, or performance claims.

Validation happens before sockets are bound, files are opened for append, or background tasks are spawned unless the setting explicitly requires runtime discovery.

### 4.4 Effective Configuration Evidence

Every validation or benchmark run should record a redacted effective configuration:

```text
key
effective value
source that supplied it
validation status
redaction status
```

Secrets are never printed by default. A pass-through mode may exist for local debugging, but it must be explicit, named, and recorded.

## 5. Error And Public Text

Errors need two separate views:

| View | Audience | Content |
|---|---|---|
| Internal error | developers/operators | precise error class, source, context, safe fields |
| Public text | client/user/terminal | stable, non-secret, compatibility-aware wording |

Rules:

- Do not let raw Rust errors become public response bodies.
- Do not duplicate public strings at call sites.
- Do not make the reporter infer protocol behavior.
- Do not make metrics infer error classes from rendered strings.
- Keep startup/config errors rich enough to fix the problem without reading source.

Recommended structure:

```text
error.rs        typed internal failures
public_text.rs  safe stable external strings
outcome.rs      feature-owned public result mapping, when a feature needs one
```

`anyhow` is acceptable at the binary boundary for contextual startup failure. Core modules should return typed errors so tests can match behavior without parsing strings.

## 6. Reporting, Logging, Metrics, And Console Output

Reporting is the end-to-end subsystem from call-site submission to selected outputs. Logging, console text, metrics, and benchmark rows are outputs or consumers, not separate unrelated call-site families.

### 6.1 Call-Site Contract

Preferred call site shape:

```rust
reporter.record(
    component,
    phase,
    step,
    severity,
    fact,
    fields,
);
```

The concrete API can be adjusted for Rust ergonomics, but the call site must visibly state:

- component or functional area;
- phase;
- step;
- severity;
- fact name;
- typed fields.

Avoid a proliferation of per-module facades such as one logger per subsystem. Per-component filtering belongs in configuration and backend routing, not in dozens of distinct call-site APIs.

### 6.2 Field Typing

Fields must carry primitive type and rendering policy:

| Type | Use |
|---|---|
| string | identifiers, labels, finite text |
| integer | counts, lengths, codes |
| float | rates and ratios |
| bool | flags |
| duration | elapsed time with explicit unit |
| bytes | byte counts with explicit unit |
| secret | redacted unless pass-through is explicitly enabled |

Domain modules submit fields. Reporter does not look up network peer addresses, route names, parser status, or sink state on its own.

### 6.3 Backends

Initial outputs:

- `tracing` JSON or compact text to stderr;
- optional console/plain output for local interactive runs;
- in-memory counters exposed by an existing inspection route or test hook;
- benchmark ledger files written by scripts.

Future outputs:

- Prometheus/OpenMetrics endpoint;
- structured run artifacts for long benchmarks;
- external notification or incident channels.

`tracing` is the first backend because it is the standard Rust structured-event path and supports target/module filtering through `tracing-subscriber::EnvFilter`. It is not the entire reporting architecture.

### 6.4 Per-Component Filters

Configuration must support a global default and overrides by component or target:

```toml
[observe]
level = "info"
format = "json"
console = false
redacted_text = "<redacted>"
allow_secret_passthrough = false

[observe.components]
network = "debug"
body = "info"
store = "warn"
```

The exact component names belong to the implementation. The infrastructure requirement is that filtering is runtime configuration, not compile-time feature selection.

## 7. Metrics

Metrics should be explicit and mechanically tied to state changes.

Rules:

- A counter increment must name the condition it records.
- Gauges must define ownership and update points.
- Histograms must define unit and bucket rationale.
- Labels must have bounded cardinality unless explicitly approved.
- Metrics and logs may share facts, but one must not silently synthesize the other.

Initial backend can be simple atomics and snapshots. The API should not prevent later export through `metrics`, Prometheus, or OpenTelemetry.

## 8. Lifecycle And Runtime Policy

### 8.1 Startup

Startup sequence:

```text
load config
validate config
initialize reporting
emit redacted effective config when requested
initialize metrics
open required sinks/files
bind network listeners
start serving tasks
wait for shutdown signal
drain and close
emit shutdown summary
```

Do not spawn background tasks before configuration and reporting are initialized enough to explain failures.

### 8.2 Shutdown

Shutdown must define:

- signal sources;
- whether new work is refused;
- how in-flight work is drained or aborted;
- maximum drain time;
- sink flush behavior;
- final metric/report summary;
- process exit code.

### 8.3 Tokio And Blocking Work

Tokio should run I/O tasks. CPU-heavy parse, compression, indexing, or database work should not be hidden inside arbitrary async handlers once measured cost becomes material.

Rules:

- keep request/I/O tasks bounded;
- use explicit queues for work transfer;
- treat `spawn_blocking` as a temporary bridge, not a general CPU scheduler;
- record worker/thread counts in benchmark ledgers;
- justify a separate CPU runtime or thread pool with measurement.

## 9. Validation And Benchmark Ledgers

Validation artifacts must be reproducible without trusting a prose summary.

Each validation run should capture:

- command line;
- config file and effective config;
- binary build/profile/git state when available;
- fixture names and generated payload summary;
- raw responses;
- application logs;
- stats snapshots before and after;
- system stats when performance is measured;
- pass/fail summary with exact comparison criteria.

Benchmarks additionally capture:

- client tool and version;
- concurrency, connection reuse, request count, payload size, compression;
- CPU model, core count, power mode, OS version;
- process memory, file descriptors, thread count;
- per-stage throughput and latency where available.

## 10. Security Defaults

Security-sensitive infrastructure rules:

- redact secrets by default in logs, config dumps, panic contexts, and test artifacts;
- separate public text from internal error detail;
- bound memory, body, decode, parser, and queue work where the owning subject defines a limit;
- log enough to diagnose hostile inputs without echoing payloads by default;
- make pass-through debugging explicit and auditable;
- fail closed for invalid configuration.

## 11. Implementation Sequence

The infrastructure sequence should support feature work without turning into ceremony:

1. Typed configuration with source precedence, validation, and redacted effective-config output.
2. Typed errors and public text separation at startup and request boundaries.
3. Reporting call-site API with `tracing` backend, console option, redaction, and component filters.
4. Metrics snapshot API with explicit counters/gauges/histograms.
5. Validation ledger conventions used by compatibility scripts.
6. Benchmark ledger conventions used by load scripts and system monitors.
7. Lifecycle drain policy and final shutdown summaries.
8. Service packaging only after process behavior is stable.

## 12. References

### Local Pattern Sources

- `/Users/walter/Work/Spank/infra/Infrastructure.md` — cited only for layered infrastructure framing and scale-regime separation.
- `/Users/walter/Work/Spank/spank-py/Infra.md` — cited only for the reusable section cadence that ties problem, decision, call-site shape, and validation together.
- `/Users/walter/Work/Spank/spank-rs/research/Infrust.md` — cited only for Rust infrastructure library candidates and validation concerns.

### External Project And Standards References

- [clap documentation](https://docs.rs/clap/latest/clap/) — CLI parsing and typed argument definitions.
- [figment documentation](https://docs.rs/figment/latest/figment/) — layered configuration providers and extraction into typed structs.
- [Serde documentation](https://serde.rs/) — typed serialization/deserialization used by config and structured output.
- [thiserror documentation](https://docs.rs/thiserror/latest/thiserror/) — derive-based typed error definitions.
- [anyhow documentation](https://docs.rs/anyhow/latest/anyhow/) — contextual top-level binary errors.
- [tracing documentation](https://docs.rs/tracing/latest/tracing/) — structured application events.
- [tracing-subscriber documentation](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/) — filtering, formatting, and subscriber setup.
- [Tokio runtime builder](https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html) — runtime thread and blocking-pool configuration.
- [Tokio `spawn_blocking`](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html) — blocking bridge behavior and cancellation caveats.
- [Twelve-Factor App: Config](https://12factor.net/config) — environment-based configuration motivation; used as background, not as a complete design.
