# Restart — HECpoc Context Handoff

Date: 2026-05-04.

Status: retired context snapshot under `docs/`. Useful current implementation state and validation evidence were moved into `/Users/walter/Work/Spank/HECpoc/InfraHEC.md`. Use this file only to understand what the restart handoff contained at the time it was retired.

## Charter

`Restart.md` is a compact context handoff. It preserves current project truth, implemented state, validation evidence, next phase, and a few operational guardrails after context loss. It is not a full design document, retrospective, task tracker, or historical archive.

## Current Direction

At retirement time, HECpoc was a Rust-only focused Splunk HEC-compatible receiver. The documentation split recorded by this snapshot was:

- `HECpoc.md` — product scope, protocol/event semantics, sink/inspection semantics, product open decisions.
- `InfraHEC.md` — infrastructure implementation spine: config, reporting, errors/outcomes/public text, lifecycle, validation, benchmarking, build/service operation.
- `Stack.md` — network ingress/TCP/Tokio/Axum/Hyper/body/buffering/copy/context-switch/kernel/backpressure technical ledger.
- `PromptMiss.md` — prompt/process miss analysis and cooperation guardrails; not product design.
- `docs/History.md` — non-authoritative abandoned attempts and evidence pointers.
- `docs/PerfIntake.md` — historical/supporting performance distillation.

Deleted duplicate docs: `Config.md`, `ErrorMessaging.md`. Their useful material was concentrated into `InfraHEC.md`.

## Implemented Code State

Implemented configuration phase:

- `clap` CLI support.
- `figment` provider chain.
- `thiserror` config errors.
- Precedence: compiled defaults < TOML config file < CLI flags < environment variables.
- `--config`, `--show-config`, `--check-config`.
- Redacted effective config TOML.
- Validation for token, address, byte/event limits, duration bounds, gzip buffer range, env parse failures.
- Tests for file load, CLI/env precedence, env config path, show/check config actions, invalid decoded limit, empty token, invalid numeric env.

Relevant files:

- `Cargo.toml` — added `clap`, `figment`, `thiserror`.
- `src/hec_receiver/config.rs` — new config implementation.
- `src/main.rs` — config action handling.
- `src/hec_receiver/protocol.rs` — removed old env parsing.
- `src/hec_receiver/mod.rs` — exports `ConfigAction`.

## Validation Evidence

Saved under `results/`:

- `test-list-20260504T021751Z.txt` — 34 tests listed.
- `test-output-20260504T021751Z.txt` — 34 tests passed.
- `check-config-20260504T021804Z.log` — `--check-config` output.
- `show-config-20260504T021804Z.log` — redacted `--show-config` output.
- `startup-20260504T021804Z.log` — current startup status output.
- `bench-ab-single-20260504T021828Z.txt` — local `ab -n 1000 -c 1` smoke output.
- `bench-ab-c50-20260504T021828Z.txt` — local `ab -n 5000 -c 50` smoke output.
- `bench-stats-20260504T021828Z.json` — stats after smoke runs.

Smoke results are not capacity claims. They only prove the receiver starts, accepts raw HEC traffic locally, and counters remain clean under small release-mode `ab` runs.

## Important Design Correction

Do not scatter direct backend-specific `tracing::info!`, `tracing::warn!`, stats updates, terminal writes, and benchmark ledger writes through all modules for product-significant events.

Current reporting direction is one structured reporting model: static report definitions plus runtime fields emitted through a reporter.

```rust
report.emit(HEC_AUTH_TOKEN_INVALID
    .at(&ctx)
    .field("auth_scheme", parsed.scheme())
    .field("token_present", parsed.token_present())
    .duration("auth_us", started.elapsed()));

report.emit(HEC_SINK_COMMIT_FAILED
    .at(&ctx)
    .state("sink_state", SinkState::CommitAttempted)
    .error_class(error.class())
    .field("sink_kind", sink.kind()));
```

Definitions carry stable name, phase, component, step, default severity, kind, outcome class, counter effects, routing, and redaction policy. Rejected and failed outcomes are classes on the same report model, not separate messaging APIs. Diagnostics are allowed but must use the same reporting machinery; they cannot be fully enumerated at design time.

Avoid `message` as the root concept. Distinguish occurrence, report definition, report record, outcome, error, output sink, renderer/backend, and public text. Output text is a rendering result, not the source of truth.

Stdout/stderr/terminal/files are output sinks. Terminal output is separate only for interactive command/UI behavior or direct CLI command responses, not because a terminal is intrinsically different from other file descriptors.

The reporter internally routes to logs/tracing, counters, status output, and benchmark ledgers where configured. `tracing` remains the first backend, not the app-level observability API.

See `InfraHEC.md §9` and `InfraHEC.md §20`.

## Next Implementation Phase

Implement Phase 2 from `InfraHEC.md §20`:

1. Add `src/hec_receiver/report.rs`.
2. Define `Reporter`, `ReportDef`, `ReportRecord`, `Phase`, `Component`, `Step`, `ReportKind`, `OutcomeClass`, `Severity`, and redaction policy.
3. Add `[observe]` config: `level`, `format`, `redaction_mode`, `redaction_text`, source filters, output toggles.
4. Add `tracing` and `tracing-subscriber` as backend dependencies.
5. Replace startup `eprintln!` with reporter/output calls.
6. Add reporter hooks at request outcome boundaries through static definitions, but avoid over-instrumenting hot event loops.
7. Keep HEC client-visible outcomes centralized and separate from internal errors and reporting events.
8. Do not add a generic `messages.rs` unless implementation proves a separate public text/rendering home is needed.
9. Add tests for report definition names, redaction, config validation, outcome class mapping, counter effects, output routing, and disabled diagnostics. Process-test actual log output after first implementation.

## Watchouts

- Do not create `hec_log_info`, `tcp_log_warn`, `queue_log_debug`, etc.
- Do not add a vague generic `msg(subsystem, level, ...)` API that loses domain event meaning.
- Do not create separate `reject`, `fail`, `measure`, or `terminal` messaging APIs unless a later implementation proves the single report-record model cannot carry the case cleanly.
- Do not treat compile-time log filtering as the main design. Runtime/config tuning matters more for this project.
- Keep `Stack.md` as deep network/Tokio/HTTP mechanics; do not move its kernel/buffer/copy/context-switch details into `InfraHEC.md`.
- Keep `HECpoc.md` product/protocol focused.
- Session/process miss analysis lives in `PromptMiss.md`; do not expand it here.
