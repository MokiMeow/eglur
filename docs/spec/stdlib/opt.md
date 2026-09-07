# `std::opt` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Type

`Opt[T]` has variants `Some(T)` and `None`.

### Constructor Typing

`Some` and `None` are variant constructors (not ad-hoc syntax sugar).

1. `Some` MUST type-check as `T -> Opt[T]`.
2. `None` MUST type-check as polymorphic `Opt[T]` for any `T`.
3. Type parameter `T` for `None` MUST be resolved from expected-type context (annotation, callee signature, or surrounding expression constraints).
4. If no context can resolve `T`, compilation MUST fail with the applicable inference/underdetermined binding diagnostic from `../language/type-system.md` (`BINDING_TYPE_UNDETERMINED` or `TYPE_ARG_INFERENCE_FAILED`).

### Equality

`Opt[T]` implements `Eq` iff `T` implements `Eq` and the instantiation is
ownership-free and reference-free:

1. `None` equals `None`.
2. `Some(a)` equals `Some(b)` iff `Eq.eq(&a, &b)` is true.
3. `None` and `Some(_)` are unequal.
4. Equality between different `Opt` instantiations MUST fail with `COMPARISON_TYPE_MISMATCH`; implementations MUST NOT insert implicit casts between payload types.
5. `Opt[Own[T]]` does not implement `Eq` in v0.1.

### Hashing

`Opt[T]` implements `Hash` iff `T` implements `Hash` and the instantiation is
ownership-free and reference-free:

1. Hashing starts with the caller-supplied seed and hashes the variant ordinal as a `u64`: `None` is `0` and `Some` is `1`.
2. `None` returns the result of hashing its variant ordinal.
3. `Some(value)` passes the variant-hash result as the seed to `Hash.hash(&value, seed)`.
4. `Opt[Own[T]]` does not implement `Hash` in v0.1.


### Required Operations (pure)

```eglur-catalog
fn Opt[T].is_some(self: &Self) -> Bool
fn Opt[T].is_none(self: &Self) -> Bool

fn Opt[T].map[U](self: Opt[T], f: callback[pure](T) -> U) -> Opt[U]
fn Opt[T].and_then[U](self: Opt[T], f: callback[pure](T) -> Opt[U]) -> Opt[U]
fn Opt[T].or_else(self: Opt[T], f: callback[pure](Unit) -> Opt[T]) -> Opt[T]

fn Opt[T].unwrap_or(self: Opt[T], default: T) -> T
fn Opt[T].unwrap_or_else(self: Opt[T], f: callback[pure](Unit) -> T) -> T

fn Opt[T].ok_or[E](self: Opt[T], e: E) -> Res[T,E]
fn Opt[T].ok_or_else[E](self: Opt[T], f: callback[pure](Unit) -> E) -> Res[T,E]
```

All callback-taking operations in this section are pure-only adapters in v0.1.
They MUST reject effectful callbacks with `PURE_COMBINATOR_EFFECTFUL_CALLBACK`; no `map!`, `and_then!`, `or_else!`, `unwrap_or_else!`, or `ok_or_else!` baseline sibling is required.

All by-value operations in this section require ownership-free, reference-free type arguments.
For the special conditional root `Opt[Own[T]]`, v0.1 guarantees only `is_some`/`is_none` borrowing queries plus constructors, `move`, `match move`, and `?` from `../language/ownership-boundary.md`.

For the fallback signatures above, `callback[pure](Unit) -> X` denotes a unary callback whose single argument has type `Unit`.
The canonical trailing-closure surface is therefore `{ |_| ... }`; zero-parameter fallback closures are not the v0.1 canonical surface for these APIs.

Semantics:

1. `map` MUST call `f` exactly once for `Some(value)` and return `Some(f(value))`; it MUST return `None` without calling `f` for `None`.
2. `and_then` MUST call `f` exactly once for `Some(value)` and return the `Opt[U]` produced by `f`; it MUST return `None` without calling `f` for `None`.
3. `or_else` MUST return the original `Some(value)` without calling `f`; it MUST call `f` with `()` exactly once for `None` and return the `Opt[T]` produced by that call.
4. `unwrap_or` MUST return the contained value for `Some(value)` and `default` for `None`.
5. `unwrap_or_else` MUST return the contained value for `Some(value)` without calling `f`; it MUST call `f` with `()` exactly once for `None` and return the value produced by that call.
6. `ok_or` MUST return `Ok(value)` for `Some(value)` and `Err(e)` for `None`.
7. `ok_or_else` MUST return `Ok(value)` for `Some(value)` without calling `f`; it MUST call `f` with `()` exactly once for `None` and return `Err` containing the value produced by that call.
