# Code-Channel Policy

Scope: Normative for v0.1 unless this file or a section states otherwise.

Diagnostic `code` identifies failure cause. Emission channel identifies when/how the cause is surfaced to users/tests.

Channels used by conformance:

1. `compile_error`
2. `runtime_error`
3. structured value-path diagnostics (`value_diagnostics`, including optional warnings/notes attached to successful `value` outcomes)

Rules:

1. Codes defined in `compile-codes.md` and `extension-codes.md` are compile-channel codes and MUST be valid in `compile_error`.
2. Codes defined in `runtime-codes.md` are cause codes and are not runtime-exclusive by default.
3. If a runtime-codes cause is statically decidable under `../language/validation-matrix.md`, implementations MUST emit the same cause code in `compile_error` channel (not a renamed compile-only alias), except where v0.1 baseline explicitly fixes that cause to value channel.
   1. In v0.1 baseline, `DETERMINISM_UNAVAILABLE` is fixed to value channel and MUST NOT be emitted as `compile_error`.
4. If the same cause is not statically decidable, implementations MUST surface it through runtime/value channels according to `../CONFORMANCE.md`.
5. Implementations MUST keep `code` stable across channels for the same cause.
6. In v0.1 baseline, `runtime_error` is reserved for unrecoverable runtime-fatal boundaries:
   1. explicit bang abort paths (required conformance exercises `std::test::abort!`),
   2. baseline-required implicit runtime-fatal boundaries (for example an unobserved child failure at `std::concurrency` scope exit),
   3. non-statically-decidable extern linkage/symbol-resolution failures at runtime (`EXTERN_LINKAGE_FAILED`).
7. Well-typed non-bang paths MUST NOT terminate via `runtime_error`; such attempts are compile-time effect/type violations (for example `EFFECT_VIOLATION`).

The currently exercised set of cross-channel cause-code assertions is
traceability-derived and MUST NOT be duplicated by hand in this document.
Use `tests/conformance/traceability.yaml` to inspect the concrete suite/test
evidence for each code and `pixi -q run --locked validate-traceability` to
verify that inventory freshness.
