# `std::diag` (OPEN, v0.2 preview)

Status: non-normative design record. This file cannot create v0.1 conformance
requirements.

## Purpose

`std::diag` defines a pure value channel for APIs that return a value with
warnings. Diagnostics remain explicit and side-effect free without weakening
non-bang purity.

## Core Types (preview)

```eglur-catalog
opaque type Diag
alias type WithDiag[T] = (T, Seq[Diag])
```

`Diag` records SHOULD align with `../../spec/errors/model.md` diagnostic shape where possible.

## Baseline Composition Rule (preview)

For preview baseline, `WithDiag` composition MUST preserve left-to-right order and preserve duplicates.

Given two values:

```text
(x, dx): WithDiag[X]
(y, dy): WithDiag[Y]
```

Any standard combining operation MUST append diagnostics as:

```text
dxy = dx ++ dy
```

No implicit dedupe/sort is applied by baseline composition.

## Res/Opt Interaction (preview)

Preview baseline fixes one canonical interaction:

1. Success-with-warning APIs SHOULD use `Res[WithDiag[T], E]`.
2. `WithDiag[Res[T,E]]` is not part of preview baseline.
3. Existing APIs remain unchanged; warning-carrying variants are added as explicit `*_checked` siblings.

Examples:

```text
parse_int(s: Str) -> Res[i64, ParseError]                    # existing
parse_int_checked(s: Str) -> Res[WithDiag[i64], ParseError]  # preview sibling
```

## Optional Utility Surface (preview)

```eglur-catalog
optional fn with_diag[T](x: T) -> WithDiag[T]
optional fn warn(code: Str, message: Str) -> Diag
optional fn diag_code(d: &Diag) -> Str
optional fn diag_message(d: &Diag) -> Str

optional fn append_diag[T](x: WithDiag[T], d: Diag) -> WithDiag[T]
optional fn map_diag[T, U](x: WithDiag[T], f: callback[pure](T) -> U) -> WithDiag[U]
optional fn zip_diag[A, B](a: WithDiag[A], b: WithDiag[B]) -> WithDiag[(A, B)]

optional fn diags[T](x: &WithDiag[T]) -> Seq[Diag]
optional fn value[T](x: &WithDiag[T]) -> T

optional fn dedupe_diags[T](x: WithDiag[T]) -> WithDiag[T]
optional fn sort_diags[T](x: WithDiag[T]) -> WithDiag[T]
```

`dedupe_diags`/`sort_diags` are explicit opt-in helpers and MUST NOT change baseline append semantics.

Utility behavior recommendations:

1. `dedupe_diags` SHOULD preserve first-occurrence order (stable dedupe).
2. `sort_diags` SHOULD use a deterministic canonical key and MUST be documented by implementation.

## Conformance Status

`std::diag` preview behavior is tracked as optional conformance only (v0.2+ candidate).

v0.1 required suites do not depend on this module.

## Open Decisions

1. `Diag` canonical payload/profile for value-channel use (full parity with compile/runtime diagnostic record vs reduced payload form).
2. Whether `WithDiag` receives dedicated sugar/combinators beyond library helpers.
3. Whether preview APIs stay in `std::diag` or selected helpers are re-exported by `std::prelude` in a later version.
