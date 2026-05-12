# RustEx — Rust Idiom And Architecture Notes

This document captures Rust-specific examples, idiom pressure, and ecosystem habits that can distort architecture choices if copied uncritically.

Mandate: keep Rust syntax, crate, macro, trait, logging, and abstraction notes separate from the product and subsystem specifications. RustEx may explain why a Rust trope is risky or useful, but implementation decisions belong in the owning design document and code review.

## 1. Rust Observations And Examples

This section captures the influence of Rust examples, idioms, and ecosystem defaults on model behavior. It is not a rejection of Rust. It is a warning that Rust teaching examples and crate documentation often optimize for local clarity, not system architecture.

### 1.1 Direct Logging Macros

Canonical Rust examples often show:

```rust
tracing::info!("server started");
tracing::warn!("invalid token");
```

Risk:

- direct backend calls spread through product code;
- severity, source, counters, redaction, public text, and benchmark routing drift apart;
- later output changes require call-site churn.

Preferred project direction:

```rust
report.emit(HEC_AUTH_TOKEN_INVALID
    .at(&ctx)
    .field("auth_scheme", parsed.scheme())
    .outcome(&outcome));
```

### 1.2 Trait And `dyn` Gravity

Canonical examples often introduce traits early:

```rust
trait Sink {
    fn write(&self, event: Event) -> Result<()>;
}

type SharedSink = Arc<dyn Sink + Send + Sync>;
```

Risk:

- a trait boundary appears before interchange requirements are proven;
- `Arc<dyn Trait>` can hide ownership, lifetime, scheduling, and performance questions;
- module layout starts following Rust demonstration shape rather than system phase/component boundaries.

Preferred project approach:

- begin with concrete structs where the first sink/queue path is still being discovered;
- introduce traits when at least two real implementations, tests, or replacement seams exist;
- keep hot-path ownership and allocation visible.

### 1.3 Enum Localism

Canonical examples often encourage local enums and `match`:

```rust
enum AuthResult {
    Missing,
    Invalid,
    Ok,
}
```

Risk:

- local enums can disconnect protocol outcomes, report definitions, counter labels, and internal errors;
- "one enum per module" can multiply translation points.

Preferred project approach:

- distinguish internal error, HEC outcome, report definition, and counter reason;
- centralize mappings at adapter boundaries;
- test mappings directly.

### 1.4 Tower And Middleware Defaults

Rust web examples often imply:

```rust
Router::new()
    .layer(RequireAuthorizationLayer::bearer("token"))
    .layer(RequestBodyLimitLayer::new(max));
```

Risk:

- middleware behavior may not match Splunk HEC compatibility;
- auth errors, gzip failures, body limits, and timeout behavior may bypass HEC response mapping;
- hostile input semantics become hidden in library defaults.

Preferred project approach:

- use Axum as a thin HTTP adapter;
- own protocol-critical auth, gzip, body limit, error mapping, and hostile-input handling;
- keep Hyper/hyper-util fallback available if accept-loop or body behavior requires it.

### 1.5 Module Fragmentation

Canonical Rust projects often split many small files early.

Risk:

- premature file layout can freeze weak concepts;
- module names become architectural claims;
- the user must track too many documents/files before the core design is stable.

Preferred project approach:

- group by phase/component/step and internal completeness;
- split only when the boundary makes code review or testing clearer;
- avoid generic names such as `messages.rs` until the real responsibility is known.

### 1.6 Standard Crate Boundary Leakage

Canonical Rust examples often import standard ecosystem crates directly in every module:

```rust
use axum::{extract::State, response::IntoResponse, Json};
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
```

This is understandable in examples because each crate is teaching its own surface. It becomes risky in a system because crate APIs start acting as the architecture:

- `axum` extractor and response types leak into protocol and domain modules;
- `serde_json::Value` becomes the event model instead of an adapter representation;
- `tokio::mpsc` becomes the queue contract before backpressure policy is specified;
- `tracing` macros become the reporting API;
- library error types leak into HEC outcome mapping and public text decisions.

Preferred project approach:

- isolate third-party crate APIs at adapter boundaries when they encode transport, runtime, rendering, or persistence choices;
- allow direct crate usage in leaf modules when the crate is the actual implementation detail and not the subsystem contract;
- keep HEC protocol, event, outcome, report definition, queue policy, and sink semantics in project-owned types;
- make imports reveal dependency direction: adapters depend on domain/protocol types, not the reverse.

Example:

```rust
// adapter layer
async fn post_raw(State(app): State<AppState>, headers: HeaderMap, body: Body) -> impl IntoResponse {
    hec_receiver::handle_raw(app.runtime(), RequestParts::from_axum(headers), body).await
}

// receiver/protocol layer
async fn handle_raw(runtime: &RuntimeState, request: RequestParts, body: BodyStream) -> HecResponse {
    // HEC-owned outcome/reporting/queue policy lives here.
}
```

This does not mean wrapping every crate for ceremony. It means resisting the canonical-example habit of letting every crate's types flow through the whole codebase before boundaries are understood.
