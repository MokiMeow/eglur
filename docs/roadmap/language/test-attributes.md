# Test Attribute Extension Roadmap (v0.2+)

Status: non-normative design record. This file cannot create v0.1 conformance
requirements.

This document defines the staged extension surface for test attributes beyond v0.1 `#[test]` and `#[ignore]`.

v0.1 required conformance excludes all items in this document.

## Goals

1. Extend test ergonomics without weakening purity/effect rules.
2. Keep deterministic reporting semantics under parallel execution.
3. Preserve clear channel separation (`value` vs `runtime_error`) in conformance.

## Stage T0 (recommended first extension set)

Stage T0 introduces only:

1. `#[should_err]`
2. `#[should_runtime_error(CODE)]`
3. `#[timeout(ms = <int>)]`
4. `#[case(...)]` (parameterized tests, literal-case only)

Explicitly excluded in Stage T0:

1. `#[should_panic]`
2. fixture providers
3. generator-based parameterization
4. tag/filter metadata attributes

## Attribute Contracts (T0)

### `#[should_err]`

1. Target MUST be a top-level function also annotated `#[test]`.
2. Intended for tests returning `Res[Unit, E]`.
3. Test passes only when the function returns `Err(_)`.
4. Returning `Ok(())` is a test failure.
5. `#[should_err]` and `#[should_runtime_error(...)]` are mutually exclusive on the same function.

### `#[should_runtime_error(CODE)]`

1. Target MUST be a top-level function also annotated `#[test]`.
2. Test passes only when execution terminates via unrecoverable runtime error with matching primary error code.
3. Normal return (`Unit` or `Res`) is a test failure.
4. This attribute is for runtime-error channel assertions only and MUST NOT reinterpret `Res::Err` as runtime failure.
5. `#[should_runtime_error(...)]` and `#[should_err]` are mutually exclusive on the same function.

### `#[timeout(ms = <int>)]`

1. Target MUST be a top-level function also annotated `#[test]`.
2. Timeout applies to the whole test invocation wall-time budget.
3. Expiration is reported as test failure with deterministic timeout diagnostics.
4. Timeout budget value MUST be positive.
5. Timeout behavior MUST NOT relax language effect rules.

### `#[case(...)]`

1. Target MUST be a top-level function also annotated `#[test]`.
2. Stage T0 allows literal-case parameterization only:
   1. scalar literals,
   2. tuple literals composed of literals,
   3. enum constructors with literal payloads where statically resolvable.
3. No fixture references, generators, or runtime data providers in Stage T0.
4. Each `#[case(...)]` expands one deterministic test instance.
5. Case expansion order MUST follow source declaration order of `#[case(...)]` attributes.

## Determinism and Reporting (T0)

1. Discovery order of test functions remains:
   1. canonical module/file lexical path,
   2. declaration source order.
2. Within one parameterized function, expanded case order MUST match `#[case(...)]` source order.
3. Runner MAY execute tests/cases in parallel.
4. Result reporting order MUST remain deterministic:
   1. function discovery order primary,
   2. case expansion order secondary.

## Effect-Model Compatibility (T0)

1. Test attribute extensions do not change effect typing:
   1. effectful tests still require bang functions,
   2. non-bang tests remain under non-bang restrictions.
2. Attributes are metadata constraints and MUST NOT authorize otherwise-forbidden effects.

## Candidate Diagnostic Families (OPEN)

Exact code names are open for standardization, but Stage T0 requires deterministic diagnostics for:

1. unsupported target use of test extension attributes,
2. conflicting expectation attributes on one test,
3. invalid timeout argument,
4. invalid/non-literal `#[case(...)]` payload.

## Open Questions

1. Whether timeout should have profile-specific defaults or remain per-test explicit only.
2. Whether case IDs should be auto-numbered only or also support explicit case labels.
3. Which subset of fixture support, if any, should be introduced in post-T0 stages.
