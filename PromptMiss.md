# PromptMiss — Session Misinterpretation Analysis

Date: 2026-05-04.

This document records recurring misses between user intent, model interpretation, and resulting artifacts during recent HECpoc, spank-rs, and spank-py design sessions. It is a process and communication artifact, not a product requirement, protocol specification, or implementation plan.

Use it to improve future Developer/Agent cooperation:

- before large documentation rewrites;
- before introducing abstractions;
- before encoding Rust ecosystem patterns as architecture;
- before preserving or relocating historical material;
- when a new context needs to understand why certain shortcuts are explicitly rejected.

---

## 1. Core Pattern

The repeated failure mode was not lack of activity. It was premature closure: the model often converted broad, contested, or exploratory instructions into familiar documentation or Rust implementation shapes before the intended level, boundary, or abstraction had stabilized.

Representative prompt:

> "Start with deep investigation of the material so far, to understand the takes broadly and to produce fresh insights."

Common model outcome:

> "technical comparisons and Rust coding"

User correction:

> "did I ask to rehash technical comparisons and Rust coding or to map out the entire effort and its apparent challenges as shown by the aborted attempts?"

Interpretation:

- The model over-weighted the nearest technical artifacts and under-weighted the higher-level product/ecosystem investigation.
- The model treated "deep investigation" as permission to produce broad technical synthesis, but the intended center was the shape, friction, and lessons of the whole effort.
- Future mitigation: create an interpretation checkpoint before writing an authoritative document.

---

## 2. Document-Type Confusion

### 2.1 Transcript Versus Study Guide

Prompt:

> "save entire session, prompts and responses as Markdown suitable for study"

Later correction:

> "did I ask for a session transcript or a study guide? generate transcript RustTrans.md but format for reading"

Likely model outcome:

> a study-guide style rewrite rather than a transcript-preserving artifact.

Miss:

- "suitable for study" was over-weighted.
- "prompts and responses" was under-weighted.

Mitigation:

- When asked to save a session, explicitly classify the requested artifact as one of: transcript, curated study guide, chronology, decision log, or restart handoff.
- If more than one is plausible, produce a two-line confirmation before writing.

### 2.2 Restart Versus Retrospective

Prompt:

> "save state of the design and next steps in firming-up specifications in relevant documents, prepare for context restart"

Model direction after corrections:

> `Restart.md` became a compact handoff with current direction, implemented code state, validation evidence, design correction, next phase, and watchouts.

Risk:

- Restart can easily become a dumping ground for history, rationale, and postmortem analysis.

Current decision:

- The valuable implementation state and validation evidence were moved into `InfraHEC.md`.
- The handoff snapshot was retired to `docs/Restart.md`.
- Put session-miss analysis here in `PromptMiss.md`, not in restart or design files.

---

## 3. Level And Cadence Mismatch

### 3.1 Three.md Strategic Scope

Prompt:

> "investigate Splunk features, requirements, ecosystem, and adjacent projects in order to prototype some OSS and commercial products in competitive or complementary spaces"

Correction:

> "Assume higher level perspective and broader outlook"

Likely model outcome:

> Rust/performance/code-centric treatment shadowed feature, benefit, ecosystem, compatibility, and competitive-priority questions.

Miss:

- The model gravitated toward implementation because local repos and Rust code were concrete.
- The user wanted product/ecosystem orientation first, then requirements and architecture.

Mitigation:

- For strategic documents, require an outline with first-level sections such as "customers/users", "capability landscape", "competitive pressure", "feature bundles", "architecture implications", and "implementation consequences".
- Do not let implementation sections appear before the product/feature drivers are established.

### 3.2 HECpoc Bloat And Premature Process

Prompt:

> "create HECpoc.md ... reference existing documentation and decisions and avoid repetition, but step us through the process properly to finally arrive at a solid starting point"

Correction excerpts:

> "HECpoc ended-up long and getting to the meat take a long while"

> "There may be too many sections"

> "between ## 4. to ## 11. we may have too many slices and perspectives, with potential divergence and beyond my cognitive range"

Miss:

- The model interpreted "step us through the process properly" as a large process scaffold.
- The user wanted a reference structure after the problem and approach were sufficiently revealed, not a many-axis project-management template.

Mitigation:

- Before expanding a document, define its reader task in one sentence.
- Use hierarchy to group related perspectives instead of serially adding more sections.
- Keep task decomposition separate from project reference documentation unless explicitly requested.

---

## 4. Authority And Location Misses

### 4.1 Config Strategy

Prompt:

> "where uniform, comsistent, and compact configuration strategy with file, env, command line configuration and defaults and lists of parameters, arguments, flags?"

Correction:

> "disagree with hand-coded config processing, when we know we will be growing toward a production implemenentation infrastructure"

Outcome:

> The implementation was corrected toward `clap`, `figment`, `serde`, TOML, validation, `--show-config`, and `--check-config`.

Miss:

- The model drifted toward hand-coded merge logic because it was locally easy.
- Prior discussion of configuration-library choices was not treated as binding enough.

Mitigation:

- Any recurring infrastructure subsystem must name the intended library strategy before implementation.
- If the model proposes custom code, it must justify why a library is unsuitable.

### 4.2 InfraHEC Versus Config/ErrorMessaging

Prompt:

> "All their material should be captured in InfraHEC"

Correction:

> "Why Config and ErrorMessaging updated? All their material should be captured in InfraHEC. What remains and why?"

Miss:

- The model updated documents that were supposed to be consolidated away.
- It did not first establish document authority and retirement status.

Mitigation:

- Maintain a document-authority table before editing.
- For retired docs, either delete them, move them to historical storage, or add only a single tombstone pointer.

### 4.3 Stack Versus InfraHEC

Correction:

> "Stack.md is specifically Network ingress/TCP/Tokio/HEC issues of processing, buffering, copies, content validation, sizes, drops, fixes, context switches and treading. why most coverage in Infra>"

Miss:

- Cross-cutting infrastructure and network-stack details blurred.
- The model moved or summarized low-level TCP/Tokio material into the infrastructure document where it increased cognitive load.

Mitigation:

- Keep `Stack.md` as the deep technical ledger for network/HTTP/Tokio/body mechanics.
- Keep `InfraHEC.md` as the cross-cutting implementation spine.
- Cross-reference specific subsections rather than copying detail.

---

## 5. Abstraction Misses

### 5.1 Reporting: Five Families Instead Of One Model

Prompt:

> "not insisting that measurement/benchmarking or complex inter-process or inter-system notifications are identical and hidden under the same site as routine system logs"

> "but must reduce some 5 X 8 call-site variations to only a few, clearly separated and named tightly and intuitively"

Model outcome quote:

> "Use a small reporting component with a few call-site families:"

> `report.event(...)`, `report.reject(...)`, `report.fail(...)`, `report.diag(...)`, `report.measure(...)`, `terminal.show_config(...)`

Correction:

> "replace 40 with 5 is not to come-up with 5"

Miss:

- The model converted "reduce call-site variation" into a new arbitrary taxonomy.
- The proposed functions were not harmonized, parallel, or comprehensive.
- They encoded output categories and outcome categories as API shape.

Current corrected direction:

> `report.emit(HEC_AUTH_TOKEN_INVALID.at(&ctx)...);`

The stable unit is a report definition plus runtime fields. Rejected/failed/diagnostic/performance/status distinctions become structured metadata and routing policy, not separate root call-site APIs.

Mitigation:

- For any proposed API, first show three call sites and explain how they do not grow into dozens of variants.
- Distinguish "same call-site shape" from "same backend behavior."

### 5.2 Reject Versus Fail

Correction:

> "how are reject and fail distinct at messaging level?"

Miss:

- The model treated reject and fail as separate reporting methods.
- At the messaging/reporting layer, both are occurrences with outcome classes.

Current distinction:

- rejected: intentional refusal or stop due to input, auth, policy, limit, or compatibility handling;
- failed: intended operation could not complete due to internal error, dependency failure, resource exhaustion, or violated invariant.

Mitigation:

- Do not invent API verbs for concepts that are better represented as record fields.

### 5.3 Terminal As Special

Correction:

> "segregating terminal, which under the hood just another file descriptor, from all other forms of output and reporting needs extra strong justification"

Miss:

- The model treated terminal output as a separate call-site family because it is human-facing.
- The user distinguished file descriptor output from truly interactive UI behavior.

Current distinction:

- stdout, stderr, terminal, files, and local sockets are output sinks;
- interactive command/UI behavior is separate only when it involves TTY detection, prompts, paging, color, refresh, interruption, or session semantics.

Mitigation:

- Do not separate an API by destination unless the behavior and lifecycle differ materially.

### 5.4 Diagnostics Cannot Be Fully Predicted

Correction:

> "diag is user-level scenario-specific utilization and cannot be fully anticipated with complete coverage at design-time"

Miss:

- The model implied diagnostics could be fully enumerated alongside product events.

Current direction:

- Product-significant occurrences have stable definitions.
- Investigation-specific diagnostics use the same reporter, redaction, severity, and routing machinery, but are not assumed complete at design time.

Mitigation:

- Separate stable product events from controlled diagnostics.
- Guard expensive diagnostics with `report.enabled(definition)`.

---

## 6. Prompting And Agent-Behavior Improvements

### 6.1 Interpretation Checkpoints

Use before major writing or implementation:

```text
I interpret this as:
- artifact type:
- authoritative home:
- intended reader:
- must include:
- must exclude:
- old material status:
- likely next action:
```

This should be short. It prevents large wrong drafts.

### 6.2 Call-Site-First API Design

Before naming modules or traits, require:

1. three representative call sites;
2. one hostile/corner case call site;
3. one disabled/debug path;
4. explanation of what remains stable if backend or output changes.

### 6.3 Document Authority Table

Maintain, at least locally during edits:

| Subject | Authoritative home | Cross-reference home | Historical only |
| --- | --- | --- | --- |
| HEC product/protocol | `HECpoc.md` | `Stack.md` for mechanics | prior attempts |
| Infrastructure | `InfraHEC.md` | `Stack.md` for mechanics | retired docs |
| Network/Tokio/body mechanics | `Stack.md` | `InfraHEC.md` brief refs | prior experiments |
| Prompt/model misses | `PromptMiss.md` | none in active design docs | chat transcript |

### 6.4 Ask Before Encoding Ambiguous Abstractions

Ask or present alternatives when introducing:

- new root API families;
- trait boundaries;
- public modules;
- document splits;
- status/task systems;
- output/reporting concepts;
- compatibility matrices.

### 6.5 Avoid "Helpful Completion" In Historical Context

When the user says a prior attempt is abandoned, the model should not continue mining it as a design authority. It may use it as evidence for:

- test cases;
- fixture sources;
- known bugs;
- naming warnings;
- compatibility observations;
- benchmark ideas.

