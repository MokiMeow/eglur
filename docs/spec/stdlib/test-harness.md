# `std::test` (OPTIONAL for production, REQUIRED for conformance)

Scope: Normative for v0.1 unless this file or a section states otherwise.

This module is used by the conformance harness.

```eglur-catalog
fn emit!(s: Str) -> Unit
fn abort!(code: Str, message: Str) -> Never
```

Semantics:

1. `emit!` is bang.
2. `emit!` appends `s` to an ordered side-effect log consumed by test assertions.
3. `abort!` is bang and MUST terminate the current program with a fatal runtime diagnostic.
4. In v0.1, `abort!` MUST produce runtime error code `RUNTIME_ABORT`.
5. The `code` argument is a conformance assertion slot and MUST be exact string `RUNTIME_ABORT` in required suites.
6. `abort!` MUST produce a runtime diagnostic message containing `message`.
7. `abort!` MUST set fatal diagnostic `data.source` to exact string `std::test::abort!`.
8. `abort!` is intended for conformance/runtime-harness validation and MAY be omitted in production distributions together with `std::test`.
9. Calling `abort!` from non-bang context MUST be rejected at compile time by normal effect rules (`EFFECT_VIOLATION`).
10. `abort!` has canonical result type `Never`; direct bottom compatibility may
    satisfy an independently established expected type, but evaluation never
    produces a normal value.

Language-level `#[test]`/`#[ignore]` discovery/execution contract is defined in `../CONFORMANCE.md`.
