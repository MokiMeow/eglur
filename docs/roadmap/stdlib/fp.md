# `std::fp` (OPEN, v0.2 preview)

Status: non-normative design record. This file cannot create v0.1 conformance
requirements.

## Scope

`std::fp` is the planned home for small, library-level FP combinators that do not require new language syntax.

`Reader` is the primary v0.2 preview target:

1. explicit context threading as pure values,
2. no implicit ambient context lookup,
3. no new effect authorization model.

## Candidate Reader Surface (OPEN)

```eglur-catalog
meta type Reader[Env, A]

fn Reader.pure[Env, A](x: A) -> Reader[Env, A]
fn Reader.ask[Env]() -> Reader[Env, Env]
fn Reader.asks[Env, A](f: callback[pure](&Env) -> A) -> Reader[Env, A]

fn Reader.run[Env, A](self: Reader[Env, A], env: &Env) -> A
fn Reader.map[Env, A, B](self: Reader[Env, A], f: callback[pure](A) -> B) -> Reader[Env, B]
fn Reader.and_then[Env, A, B](self: Reader[Env, A], f: callback[pure](A) -> Reader[Env, B]) -> Reader[Env, B]
fn Reader.local[EnvOuter, EnvInner, A](
  self: Reader[EnvInner, A],
  adapt: callback[pure](&EnvOuter) -> EnvInner
) -> Reader[EnvOuter, A]
```

The catalog uses `meta type Reader` because its callable-alias versus nominal-wrapper representation remains an explicit OPEN decision below.

## Rules (preview baseline)

1. `Reader` APIs are pure (non-bang).
2. `Reader.run` MUST be deterministic for equal `(reader, env)` inputs.
3. `Reader` MUST NOT introduce hidden global environment channels.
4. Any environment dependency remains explicit in type/signature surface.
5. `Reader` does not grant effect permissions; bang/non-bang legality remains governed by `../../spec/language/effects.md`.

## Design Boundary

`std::fp::Reader` is library-level abstraction only.

v0.2 preview explicitly excludes:

1. implicit context syntax (for example `with`/`use env`),
2. compiler-inserted Reader plumbing,
3. effect-capability reinterpretation of Reader environment.

## Open Decisions

1. Whether `Reader` is represented as an intrinsic callable alias (`&Env -> A`) or as a nominal wrapper type with equivalent semantics.
2. Whether a minimal law-check profile (Functor/Monad identity/associativity) is added to optional conformance for preview modules.
3. Whether selected `std::fp` helpers become `std::prelude` re-exports after stabilization.
