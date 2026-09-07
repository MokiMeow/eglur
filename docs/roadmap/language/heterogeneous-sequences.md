# Heterogeneous Sequence Roadmap (`OPEN`)

Status: non-normative design record. This file cannot create v0.1 conformance
requirements.

## Goal

Define a Python-list-like heterogeneous container surface without weakening v0.1 guarantees for `Seq[T]` and `Vec[T]`.

## Decision Summary

The v0.2 candidate uses ingress-only boxing. It remains non-baseline until
a dedicated v0.2 activation decision is made.

1. Keep internal canonical representation as homogeneous `Seq[DynVal]`.
2. Allow ergonomic ingress-only boxing surface (for example `dyn[...]`).
3. Keep all dynamic type narrowing explicit via `Res`-returning extractors.

## Candidate Models and Tradeoffs

### Explicit `DynVal` construction

Example:

```eglur
let xs = DynSeq.new()
  .push(DynVal::Int(1))
  .push(DynVal::Str("x"))
```

Pros:

1. Maximum explicitness.
2. Lowest magic at parse/typecheck boundaries.

Cons:

1. High call-site friction.
2. Poor ergonomic parity with Python-list-like usage.

### Ingress-only boxing (selected)

Example:

```eglur
let xs = dyn[1, "x", true]
let n = xs.get(0)?.as_i64()?
```

Pros:

1. Ergonomic ingress while preserving canonical homogeneous internals.
2. Clear boundary: normalize once at ingress, then operate on `Seq[DynVal]`.
3. Keeps `Seq[T]`/`Vec[T]` monomorphic guarantees intact.

Cons:

1. Requires boxing trait/intrinsic and extractor diagnostics.
2. Still requires explicit extraction and `Res` handling at use sites.

### `AnySeq` dynamic container

Example:

```eglur
xs: AnySeq = [1, "x", true]
```

Pros:

1. Most familiar surface for dynamic-language users.

Cons:

1. Leaks dynamic typing across API boundaries.
2. Increases runtime failure surface and weakens static guarantees.
3. Conflicts with v0.1 philosophy of strict internal canonical model.

## Proposed Type Model (v0.2 candidate)

```text
type DynVal = Int(i64) | UInt(u64) | Float(f64) | Bool(Bool) | Str(Str)
type DynSeq
type DynTypeError
```

Proposed representation:

1. `DynSeq` is a nominal container type whose canonical internal payload is `Seq[DynVal]`.
2. `Seq[T]` and `Vec[T]` remain homogeneous and unchanged.
3. `DynVal` remains a closed sum in the initial profile; growth is additive and versioned.

## API Boundary (v0.2 candidate)

```text
dyn[e1, e2, ...] -> DynSeq

DynSeq.new() -> DynSeq
DynSeq.len(self: &Self) -> Nat
DynSeq.get(self: &Self, i: Nat) -> Opt[DynVal]
DynSeq.push(self: DynSeq, x: DynVal) -> DynSeq

Seq[T].to_dynseq(self: Seq[T]) -> DynSeq              # requires DynBox[T]
DynSeq.to_seq[T](self: DynSeq) -> Res[Seq[T], DynTypeError]

DynVal.as_i64(self: &Self) -> Res[i64, DynTypeError]
DynVal.as_u64(self: &Self) -> Res[u64, DynTypeError]
DynVal.as_f64(self: &Self) -> Res[f64, DynTypeError]
DynVal.as_bool(self: &Self) -> Res[Bool, DynTypeError]
DynVal.as_str(self: &Self) -> Res[Str, DynTypeError]
```

Boundary policy:

1. Flexible input is accepted only at API ingress (`dyn[...]`, `to_dynseq`).
2. Internals stay canonical (`DynSeq` over `DynVal`).
3. No implicit narrowing from `DynVal` to concrete type.
4. Size and index surfaces mirror the v0.1 `Nat` policy.

## Ownership and Effect Boundary

1. `DynSeq` operations follow existing pure/bang conventions.
2. `Own[DynSeq]` is valid at root level under existing ownership rules.
3. Initial profile excludes `Own[_]` payload variants inside `DynVal` to avoid nested discharge complexity.
4. Any future `DynVal` ownership-carrying extension must specify exact-once discharge semantics before baseline adoption.

## Channel Policy (Compile vs Runtime)

Compile-time failures (candidate):

1. Use of `dyn[...]` when feature/profile is disabled.
2. `to_dynseq` when `T` lacks `DynBox[T]` support.

Runtime value-channel failures (`Res::Err(DynTypeError)`):

1. `DynVal.as_*` extractor type mismatch.
2. `DynSeq.to_seq[T]` when any element cannot be narrowed to `T`.
3. Out-of-range element access remains existing `Opt`/`IndexError` behavior depending on API.

No runtime-abort channel is used for dynamic-narrowing mismatches in baseline candidate behavior.

## Initial Conformance Plan (v0.2 candidate)

Required suites once activated:

1. Ingress boxing:
   1. `dyn[...]` preserves order and element tags.
2. Extraction:
   1. `as_*` success and mismatch `Res::Err`.
3. Conversion:
   1. `Seq[T].to_dynseq` success.
   2. `DynSeq.to_seq[T]` fail-fast mismatch with index in diagnostics.
4. Boundary:
   1. `Seq[T]`/`Vec[T]` APIs remain monomorphic and unchanged.
5. Ownership:
   1. `Own[DynSeq]` obeys exact-once discharge under existing rules.
6. Effects:
   1. non-bang dynamic operations remain pure.
