# Conformance Scope

Scope: Normative for v0.1 unless this file or a section states otherwise.

A conforming Eglur implementation MUST:

1. Implement all modules marked REQUIRED.
2. Implement all signatures and semantics marked MUST.
3. Satisfy diagnostic requirements referenced from `../ERRORS.md`.

A conforming implementation MAY provide additional modules and APIs, but MUST NOT violate:

1. Effect naming rules (`!`).
2. `Res`/`Opt`/`Seq` short-circuit and ordering guarantees.
3. Named-axes correctness constraints for tensor operations.
