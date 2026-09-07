# `std::res` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Type

`Res[T,E]` has variants `Ok(T)` and `Err(E)`.

### Constructor Typing

`Ok` and `Err` are variant constructors (not ad-hoc syntax sugar).

1. `Ok` MUST type-check as `T -> Res[T,E]`; the payload type `T` is inferred from the argument and/or expected type context, and `E` MUST be resolved from expected type context.
2. `Err` MUST type-check as `E -> Res[T,E]`; the payload type `E` is inferred from the argument and/or expected type context, and `T` MUST be resolved from expected type context.
3. Expected type context includes explicit annotations, callee signatures, return positions, and surrounding expression constraints that require a concrete `Res[T,E]`.
4. If any `Res` type parameter remains unresolved, compilation MUST fail with the applicable inference/underdetermined binding diagnostic from `../language/type-system.md` (`BINDING_TYPE_UNDETERMINED` or `TYPE_ARG_INFERENCE_FAILED`).
5. If expected type context fixes the constructor payload type and the argument is not assignable to that payload type, compilation MUST fail with `CALL_ARG_TYPE_MISMATCH`.

### Equality

`Res[T,E]` implements `Eq` iff both `T` and `E` implement `Eq` and the
instantiation is ownership-free and reference-free:

1. `Ok(a)` equals `Ok(b)` iff `Eq.eq(&a, &b)` is true.
2. `Err(a)` equals `Err(b)` iff `Eq.eq(&a, &b)` is true.
3. Values with different variants are unequal.
4. Equality between different `Res` instantiations MUST fail with `COMPARISON_TYPE_MISMATCH`; implementations MUST NOT insert implicit casts between payload types.
5. `Res[Own[T], E]` does not implement `Eq` in v0.1.

### Hashing

`Res[T,E]` implements `Hash` iff both `T` and `E` implement `Hash` and the
instantiation is ownership-free and reference-free:

1. Hashing starts with the caller-supplied seed and hashes the variant ordinal as a `u64`: `Ok` is `0` and `Err` is `1`.
2. The selected payload is then hashed using the variant-hash result as its seed.
3. `Res[Own[T], E]` and `Res[T, Own[E]]` do not implement `Hash` in v0.1.


### Required Operations (pure)

```eglur-catalog
fn Res[T,E].is_ok(self: &Self) -> Bool
fn Res[T,E].is_err(self: &Self) -> Bool

fn Res[T,E].map[U](self: Res[T,E], f: callback[pure](T) -> U) -> Res[U,E]
fn Res[T,E].and_then[U](self: Res[T,E], f: callback[pure](T) -> Res[U,E]) -> Res[U,E]
fn Res[T,E].map_err[F](self: Res[T,E], f: callback[pure](E) -> F) -> Res[T,F]
fn Res[T,E].or_else[F](self: Res[T,E], f: callback[pure](E) -> Res[T,F]) -> Res[T,F]

fn Res[T,E].unwrap_or(self: Res[T,E], default: T) -> T
fn Res[T,E].unwrap_or_else(self: Res[T,E], f: callback[pure](E) -> T) -> T

fn Res[T,E].ok(self: Res[T,E]) -> Opt[T]
fn Res[T,E].err(self: Res[T,E]) -> Opt[E]
```

All callback-taking operations in this section are pure-only adapters in v0.1.
They MUST reject effectful callbacks with `PURE_COMBINATOR_EFFECTFUL_CALLBACK`; no `map!`, `and_then!`, `map_err!`, `or_else!`, or `unwrap_or_else!` baseline sibling is required.

All by-value operations in this section require ownership-free, reference-free type arguments.
For the special conditional root `Res[Own[T], E]`, v0.1 guarantees only `is_ok`/`is_err` borrowing queries plus constructors, `move`, `match move`, and `?` from `../language/ownership-boundary.md`.

Semantics:

1. `map` MUST call `f` exactly once for `Ok(value)` and return `Ok(f(value))`; it MUST return the original `Err(error)` without calling `f`.
2. `and_then` MUST call `f` exactly once for `Ok(value)` and return the `Res[U,E]` produced by `f`; it MUST return the original `Err(error)` without calling `f`.
3. `map_err` MUST call `f` exactly once for `Err(error)` and return `Err(f(error))`; it MUST return the original `Ok(value)` without calling `f`.
4. `or_else` MUST return the original `Ok(value)` without calling `f`; it MUST call `f` exactly once for `Err(error)` and return the `Res[T,F]` produced by `f`.
5. `unwrap_or` MUST return the contained value for `Ok(value)` and `default` for `Err(error)`.
6. `unwrap_or_else` MUST return the contained value for `Ok(value)` without calling `f`; it MUST call `f` exactly once for `Err(error)` and return the value produced by `f`.
7. `ok` MUST return `Some(value)` for `Ok(value)` and `None` for `Err(error)`.
8. `err` MUST return `Some(error)` for `Err(error)` and `None` for `Ok(value)`.


### Effectful Inspection (REQUIRED)

```eglur-catalog
fn Res[T,E].tap_ok!(self: Res[T,E], f: callback[effectful](&T) -> Unit) -> Res[T,E]
fn Res[T,E].tap_err!(self: Res[T,E], f: callback[effectful](&E) -> Unit) -> Res[T,E]
```

These by-value inspection operations require ownership-free, reference-free `T` and `E` and are unavailable on the conditional root `Res[Own[T], E]` in v0.1.

Semantics:

1. `tap_ok!` calls `f` iff value is `Ok`.
2. `tap_err!` calls `f` iff value is `Err`.
3. Both MUST return original `Res` unchanged.
