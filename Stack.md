# Stack — Tokio, Axum, Hyper, And OS Ingress Mechanics

Scope: technical behavior of the HECpoc network and HTTP ingress stack: OS sockets, Tokio runtime behavior, Axum routing, Hyper request parsing, body streaming, buffering, timeouts, concurrency, and future accept-loop control. This document does not define application protocol response codes, authentication semantics, event fields, index rules, ACK behavior, or Splunk compatibility matrices; those belong in `HECpoc.md`.

Mandate: explain what the network/runtime stack does before, during, and around request handling, including which failures reach application code and which are absorbed by OS/Hyper/Axum mechanics. Stack may name stack-adjacent configuration categories, but authoritative parameter catalogs belong to the owning subject document or the configuration machinery in `InfraHEC.md`.

## 1. Boundary Rule

`Stack.md` owns mechanics below and around the HTTP handler. `HECpoc.md` owns what the handler decides those mechanics mean to a client.

| Area | Stack Owns | HECpoc Owns |
|------|------------|-------------|
| Socket bind/listen | address binding, backlog options when exposed, socket buffer options when owned | configured listen address and startup failure reporting |
| Connection acceptance | who accepts sockets, how connection counts could be collected, where peer address is available | whether to reject work, report busy, or classify source |
| HTTP parsing | Hyper/Axum behavior for method/path/header/body framing | protocol route set, error body, status/code mapping |
| Body streaming | chunk/frame read loop, byte caps, idle/total timers, read errors | endpoint-specific body meaning and response mapping |
| Content decode mechanics | gzip implementation, decode buffer, expansion cap | whether decoded content is valid application input |
| Concurrency | Tokio runtime, request tasks, future CPU/write-pool split | event validation, queue policy, commit-state truthfulness |
| Observability hooks | possible socket/request timing and byte counters | domain facts, response codes, reason labels |

Practical rule:

```text
Stack says: "Hyper rejected a malformed header before route code ran."
HECpoc says: "That condition cannot currently produce an application-owned response body."
```

## 2. Current Runtime Path

Current server path:

```text
main.rs
  -> RuntimeConfig::load()
  -> tokio::net::TcpListener::bind(addr)
  -> hec_receiver::router(state)
  -> axum::serve(listener, router)
  -> Axum route match
  -> hec_request route adapter receives Request<Body>
  -> explicit handler body-read/decode/parse pipeline
```

Implementation anchors:

| File | Stack-Relevant Role |
|------|---------------------|
| `/Users/walter/Work/Spank/HECpoc/src/main.rs` | Tokio multi-thread runtime, listener bind, Axum serve, shutdown signal |
| `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/app.rs` | Axum router assembly and known-route method handling |
| `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/hec_request.rs` | route adapters and explicit request pipeline ordering |
| `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/body.rs` | body byte limits, timeout wrappers, content-encoding parsing, gzip decode |
| `/Users/walter/Work/Spank/HECpoc/src/hec_receiver/config.rs` | configured stack-adjacent limits: body bytes, decoded bytes, timeouts, gzip buffer |

## 3. Axum And Hyper Status

Axum remains the HTTP adapter and Hyper remains the HTTP parser/server substrate reached through Axum.

This is still accurate after the latest implementation:

- `axum::serve` is simple and sufficient for the current path.
- Hyper can reject malformed HTTP framing before route handlers run.
- Direct Hyper or `hyper-util` remains the fallback if connection lifecycle, socket options, header limits, or malformed-framing behavior require lower-level control.
- Middleware may be considered later for non-protocol concerns only after it is proven not to hide body consumption, error body shape, timing, or counters.
- The current direct handler path is useful because it makes ordering inspectable: phase check, query/header checks, content-encoding check, advertised length check, body read, decode, endpoint decode, sink disposition.

## 4. Axum/Hyper Behavior That Matters

Axum routes known methods and paths, then passes `Request<Body>` to handlers. Hyper performs HTTP syntax parsing before Axum route handlers run.

Important consequences:

| Condition | Who Sees It First | Current Consequence |
|-----------|-------------------|---------------------|
| Unknown route after valid HTTP parse | Axum fallback | application can return deliberate body/status |
| Wrong method on known route | Axum method router | application can install explicit method fallback |
| Unsupported content encoding header value | handler can inspect header before body read | application-owned response and reporting possible |
| Advertised `Content-Length` over configured cap | handler can inspect header before body read | application-owned response and reporting possible |
| Actual body exceeds configured cap without useful length | handler body reader | application-owned response and reporting possible |
| Malformed HTTP header syntax | Hyper parser before route | handler does not run |
| Malformed `Content-Length` rejected by Hyper | Hyper parser before route | current response is Hyper-generated, not application-owned |
| Partial headers or slowloris before request formation | Hyper/socket layer | current application body timers do not apply |

This is the main reason an owned Hyper/hyper-util accept path may eventually be needed: not for normal application correctness, but for connection accounting, peer culling, header timeouts, header-size policy, and exact treatment of malformed framing that never becomes an Axum request.

## 5. Body Read And Decode Mechanics

Current body stages:

```text
Request<Body>
  -> inspect headers
  -> reject advertised oversize if Content-Length is usable
  -> read Body frames with max_http_body_bytes
  -> enforce idle timeout per frame wait
  -> enforce total timeout for complete body read
  -> decode identity or gzip
  -> enforce max_decoded_body_bytes
```

Configured defaults:

| Parameter | Default | Stack Use |
|-----------|---------|-----------|
| `limits.max_bytes` | `1_000_000` | max advertised and received HTTP body bytes |
| `limits.max_decoded_bytes` | `4_000_000` | max identity/gzip-expanded bytes |
| `limits.idle_timeout` | `5s` | max wait for next body frame after a request exists |
| `limits.total_timeout` | `30s` | max elapsed body-read duration after a request exists |
| `limits.gzip_buffer_bytes` | `8_192` | scratch buffer while expanding gzip |

Gaps:

- Header-read timeout and header-size limit are not exposed through the current `axum::serve` path.
- There is no independent per-line or per-envelope byte cap at the stack level.
- Body timers begin only after Hyper has produced a request and Axum has routed it.
- The gzip decoder is synchronous and runs on the request task; this is acceptable only while decoded sizes remain bounded and measured.

## 6. Tokio IO And CPU Scheduling

Current rule: keep request tasks short, bounded, and mostly I/O-oriented. Do not hide CPU-heavy parse/index/search work inside arbitrary async tasks.

| Work Class | Current Placement | Revisit Trigger |
|------------|-------------------|-----------------|
| bind/listen/serve | Tokio I/O runtime | need socket options, accept stats, culling |
| header checks and route dispatch | Axum request task | no change unless direct Hyper path is selected |
| bounded body read | request task | body read latency or fairness degrades under load |
| bounded gzip decode | request task | CPU samples show gzip dominates runtime threads |
| application body decode | request task | parser CPU dominates or delays health/body progress |
| format parsing/tokenization/indexing | not in current hot path | feature is added; move behind explicit CPU pipeline |
| file/database commit | direct sink now; queue/write path later | commit state or throughput requires isolation |

Tokio-specific cautions:

- `spawn_blocking` is not a general CPU-pool design. Tokio’s blocking pool can grow large and already-started blocking tasks cannot be aborted.
- A separate CPU runtime or dedicated thread pool is justified only after a measured parse/index workload needs sustained CPU saturation.
- I/O progress, health latency, body-read latency, and connection progress must be measured while CPU-heavy stages are loaded.
- Queue boundaries should be explicit so backpressure is observed as bounded state, not as async runtime congestion.

## 7. Future Accept Loop

The current `axum::serve(listener, app)` path is simple and correct enough for initial request behavior. Replace it only for specific mechanical needs.

Reasons to own the accept path:

| Need | Why Axum Serve Is Insufficient |
|------|--------------------------------|
| connection-current/max counters | application does not currently own per-connection lifecycle |
| peer/IP rate limits | requires per-peer state before request route |
| socket options | need `TcpSocket` or platform calls before listen/accept |
| accept backlog experiments | need explicit socket setup and OS-specific validation |
| header timeout/header-size policy | request handlers run too late |
| connection culling | need last-I/O/current-bytes state and close control |
| core affinity experiments | need controlled runtime/thread/process setup |

Candidate path:

```text
TcpSocket setup
  -> listen
  -> accept loop
  -> per-connection context
  -> hyper-util connection serving
  -> Axum Router as service or direct handler
```

Do not start here. Add it when a benchmark or attack test shows a real need.

## 8. Kernel And Platform Knobs To Track

Linux and macOS expose different visibility and tuning surfaces. Stack investigations should record both configuration and observed values.

| Topic | Linux Examples | macOS Examples | Why It Matters |
|-------|----------------|----------------|----------------|
| listen/accept | `ss`, `netstat`, `/proc/net/tcp`, `somaxconn` | `netstat`, `lsof`, `sysctl kern.ipc.somaxconn` | backlog and connection state |
| socket buffers | `net.core.rmem_max`, `net.ipv4.tcp_rmem` | `net.inet.tcp.recvspace`, `net.inet.tcp.sendspace` | receive/send buffering under burst |
| file descriptors | `ulimit -n`, `/proc/<pid>/fd` | `ulimit -n`, `lsof -p` | connection capacity |
| CPU scheduling | `taskset`, `perf`, `pidstat` | Instruments, `sample`, `powermetrics` | runtime fairness and CPU attribution |
| network errors | `ss -tin`, retransmits, drops | `netstat -s`, packet captures | client/server blame for resets |
| filesystem cache | `/proc/meminfo`, `vmtouch`, `drop_caches` | `vm_stat`, `purge`, `fs_usage` | file input and sink benchmarking |

Any tuning claim should include OS version, CPU model, power mode, command line, config, payload, and result directory.

## 9. Validation Owned By Stack

Stack validation proves mechanics, not application semantics.

| Test Class | Examples | Evidence |
|------------|----------|----------|
| live server smoke | start binary, send valid request, stop cleanly | command, logs, response, exit status |
| malformed framing | malformed `Content-Length`, partial headers, truncated chunked body | raw socket script output and server response |
| timeout behavior | header stall, body stall, slow trickle, total body budget | elapsed time, status, logs, stats |
| size enforcement | advertised oversize, actual oversize, gzip expansion oversize | response body, stats, memory |
| concurrency | many keep-alive connections, many short connections, mixed slow/fast clients | current/max connections, latency, CPU, descriptors |
| CPU interference | gzip/JSON load while health/body reads continue | health latency, body-read latency, CPU samples |

Application response matrices belong in `HECpoc.md`; Stack should only explain whether the request reached application code and what mechanical limit fired.

## Appendix A. HTTP Without Direct Tower Middleware

This appendix preserves the still-valid Tower analysis without making it part of the main stack flow.

Current conclusion:

- Axum integrates with Tower; using Axum means Tower concepts exist in the dependency graph.
- Current source does not need direct `tower::ServiceBuilder`, `tower_http::auth`, `tower_http::decompression`, `tower_http::limit`, or `tower_http::timeout` calls for current request handling.
- Direct Tower or `tower-http` middleware remains inappropriate for protocol-critical request handling until tests prove it preserves ordering, body consumption behavior, response shape, redaction, counters, and timeout semantics.
- Tower middleware may still be useful later for generic non-protocol concerns, such as outer tracing after redaction is proven or auxiliary routes if those appear.

Why this remains accurate:

- Axum documentation states that Axum integrates with Tower rather than owning a bespoke middleware system.
- Axum middleware documentation recommends `tower::ServiceBuilder` when applying multiple middleware, which confirms Tower is the normal Axum middleware substrate.
- `tower-http` deprecated the simple `require_authorization` helper as too basic for real-world applications, matching our concern that exact-header auth helpers are not a protocol-compatibility layer.

## 10. References

### Local Document References

- `/Users/walter/Work/Spank/HECpoc/HECpoc.md` — referenced only to define the boundary between stack mechanics and application-visible protocol decisions.
- `/Users/walter/Work/Spank/HECpoc/InfraHEC.md` — referenced only for shared configuration/reporting machinery; Stack owns the mechanical meaning of stack-adjacent limits and failures.

### External Project And Standards References

- [Axum crate documentation](https://docs.rs/axum/latest/axum/) — confirms Axum routing/extractor/response model, Tokio/Hyper compatibility, and the Axum/Tower relationship.
- [Axum middleware documentation](https://docs.rs/axum/latest/axum/middleware/) — confirms Tower middleware integration, ordering concerns, `ServiceBuilder` guidance, and middleware error-handling behavior.
- [Axum `serve`](https://docs.rs/axum/latest/axum/fn.serve.html) — documents the intentionally simple serving helper; lower-level control requires a different path.
- [Hyper documentation](https://docs.rs/hyper/latest/hyper/) — HTTP parser/server substrate underneath Axum.
- [hyper-util documentation](https://docs.rs/hyper-util/latest/hyper_util/) — lower-level utilities for custom connection serving if the accept loop must be owned.
- [tower-http auth module](https://docs.rs/tower-http/latest/tower_http/auth/) — documents current authorization middleware surface and deprecated simple authorization helper.
- [tower-http changelog](https://docs.rs/crate/tower-http/latest/source/CHANGELOG.md) — records the deprecation context for simple authorization helpers.
- [Tokio `TcpSocket`](https://docs.rs/tokio/latest/tokio/net/struct.TcpSocket.html) — socket construction surface for future bind/listen/socket-option experiments.
- [Tokio `spawn_blocking`](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html) — blocking pool behavior and cancellation caveats.
- [Tokio runtime builder](https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html) — worker thread and blocking-thread configuration.
- [Tokio issue 8085](https://github.com/tokio-rs/tokio/issues/8085) — current discussion context around scheduling behavior under CPU-heavy workloads.
- [Apache DataFusion issue 13692](https://github.com/apache/datafusion/issues/13692) — practical CPU/I/O runtime separation discussion relevant to ingest pipelines.
- [Linux `socket(7)`](https://man7.org/linux/man-pages/man7/socket.7.html) — socket option and buffer behavior background.
- [Linux `tcp(7)`](https://man7.org/linux/man-pages/man7/tcp.7.html) — TCP buffering, backlog, keepalive, and protocol tuning background.
- [Linux `epoll(7)`](https://man7.org/linux/man-pages/man7/epoll.7.html) — Linux readiness notification mechanism relevant to async networking.
- [Apple `kqueue(2)` man page](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/kqueue.2.html) — macOS/BSD event notification mechanism relevant to async networking.
