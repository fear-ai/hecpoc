# HECpoc Visual References

Mermaid source diagrams for HECpoc design references.

| File | Purpose |
|------|---------|
| `stage_flow.mmd` | End-to-end HEC stage flow from transport stream through `HecEvents`, disposition, commit state, inspection, and optional search preparation. |
| `hec_batch_writeblock.mmd` | Distinguishes HEC input batching, including stacked JSON objects and JSON array input, from `HecEvents`, `ParseBatch`, and `WriteBlock` store/output grouping. |

Accuracy notes:

- Rejected requests do not produce accepted `HecEvents`; `stage_flow.mmd` sends them to response/reporting outcome, not store commit state.
- `HecBatch` is limited to HEC HTTP input grouping. `WriteBlock` is store/output grouping and must not inherit shipper request boundaries.
- `ParseBatch` is optional later format/search preparation grouping, not current HTTP success semantics.
