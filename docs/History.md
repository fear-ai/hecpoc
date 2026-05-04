# History — Non-Authoritative Prior Work Notes

This file keeps prior attempts visible without letting them steer the current HECpoc design. The controlling documents are `HECpoc.md`, `InfraHEC.md`, and `Stack.md`.

## Status

Prior Python and Rust attempts are abandoned as implementation plans. They may be consulted as evidence for a specific protocol behavior, fixture source, benchmark idea, parser edge case, or naming caution.

## Useful Evidence Only

Consult older material only when a current task asks a narrow question such as:

- which HEC edge cases were already noticed;
- which local logs or tools exist;
- which parser/tokenizer benchmark ideas were explored;
- which names or structures caused confusion;
- which Splunk, Vector, or shipper compatibility cases were already identified.

Do not import old architecture, status registers, task chains, crate layout, threading model, configuration loader, error model, sink names, or hand-coded merge logic.

## Known Abandoned Directions

- Hand-coded configuration merging is not a mainstay; use `clap`, `figment`, typed `serde` config, and explicit validation.
- Hot reload is not a near-term goal; use frozen startup config.
- Prior `Sender`, `Row`, and broad `Record` vocabulary is not controlling; use event/sink/source/context terms where they fit current design.
- Broad old workspace decomposition is not controlling; begin with a focused HEC receiver crate/module layout.
- Performance harness ideas are not product architecture; they enter only through named benchmarks and agreement tests.

## Pointers

- `/Users/walter/Work/Spank/spank-rs/docs/HECst.md` — protocol investigation and edge cases to re-verify against current tests.
- `/Users/walter/Work/Spank/spank-rs/perf/Tools.md` — tools, corpora, and validation ideas.
- `/Users/walter/Work/Spank/spank-rs/perf/SpankMax.md` — performance experiment framing.
- `/Users/walter/Work/Spank/spank-py/HEC.md` — historical Python HEC notes, evidence only.
