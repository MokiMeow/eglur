# Failure and Optionality

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Core Types

`Opt[T]` SHALL provide exactly two variants: `Some(T)` and `None`.

`Res[T, E]` SHALL provide exactly two variants: `Ok(T)` and `Err(E)`.

`Opt`/`Res`, ownership constructor `Own`, layout constructor `Box`, callable
head `Fn`, structural error-union head `Union`, and intrinsic bottom type
`Never` are built-in type-namespace names in v0.x:

1. User type-namespace bindings MUST NOT introduce `Opt`, `Res`, `Own`, `Box`,
   `Fn`, `Union`, or `Never`; the complete binding-site rule is in
   `algebraic-data.md` (`RESERVED_BUILTIN_NAME` otherwise).
2. User declarations MUST NOT introduce `Some`, `None`, `Ok`, or `Err` as value constructors (`RESERVED_BUILTIN_NAME` otherwise).

`Some` and `None` are constructors of `Opt[T]`:

1. `Some` SHALL have constructor type `T -> Opt[T]`.
2. `None` SHALL have polymorphic constructor type `Opt[T]`.
3. `None` is a normal typed value constructor, not special declaration syntax.
4. `None` requires contextual type resolution for `T`; if unresolved, compilation MUST fail under local type-checking rules in `type-system.md` (for example `BINDING_TYPE_UNDETERMINED` or `TYPE_ARG_INFERENCE_FAILED`).

`Ok` and `Err` are constructors of `Res[T, E]`:

1. `Ok` SHALL have constructor type `T -> Res[T, E]`.
2. `Err` SHALL have constructor type `E -> Res[T, E]`.
3. Both constructors are normal typed value constructors, not special declaration syntax.
4. `Ok` requires contextual type resolution for `E`; `Err` requires contextual type resolution for `T`.
5. If the required contextual type is unresolved, compilation MUST fail under local type-checking rules in `type-system.md` (for example `BINDING_TYPE_UNDETERMINED` or `TYPE_ARG_INFERENCE_FAILED`).
6. If context fixes a constructor payload type and the argument is not assignable to that type, compilation MUST fail with `CALL_ARG_TYPE_MISMATCH`.
7. If `E` is an error union, `Err(e)` is valid when type of `e` is assignable to the normalized union under `type-system.md` error-union assignability rules.

Ownership-carrier constructor rules:

1. `Some(owning)` and `Ok(owning)` may form only the conditional root types `Opt[Own[T]]` and `Res[Own[T], E]` allowed by `ownership-boundary.md`.
2. An existing named direct root MUST be passed as `Some(move x)` or `Ok(move x)` (`OWNERSHIP_TRANSFER_REQUIRES_MOVE` otherwise).
3. A fresh direct-root expression may be passed without an additional `move` marker.
4. `Err` payloads MUST be ownership-free and reference-free in v0.1 (`OWNERSHIP_TYPE_POSITION_FORBIDDEN` or `REFERENCE_TYPE_POSITION_FORBIDDEN` otherwise, under ordinary type-validation precedence).


### `result {}` Block

The `result {}` form introduces a scoped short-circuit context.

The `?` operator MUST be valid only inside `result {}` blocks in v0.1.0.

Using `?` outside `result {}` MUST be a compile-time error (`Q_OUTSIDE_RESULT`).


### `checked {}` Numeric Block

The `checked {}` form is a lexical value-channel boundary for scalar fixed-width integer faults.

1. A `checked { ... }` expression evaluates to `Res[T, NumericError]`, where `NumericError` is the shared nominal error type from `std::prelude`.
2. Success typing and completion are:
   1. a final `yield expr` fixes `T` to the type of `expr`; if that expression
      completes, the boundary produces `Ok(expr)`,
   2. a block without a final `yield` fixes `T = Unit`; if it reaches normal
      fallthrough, the boundary produces `Ok(())`,
   3. a body with no normal success edge produces no `Ok` value, but its static
      checked-expression type remains the `Res[T, NumericError]` fixed by Rules
      2.1-2.2 or independently established expected context.
3. A checked scalar operation that fails with `INT_OVERFLOW`, `INT_DIV_BY_ZERO`, or `INT_REM_BY_ZERO` MUST stop evaluation of the enclosing `checked` block and produce `Err(NumericError)` while preserving that cause code and its required diagnostic data.
4. The boundary covers expressions in lexically nested ordinary blocks and control-flow expressions within its body.
5. The boundary MUST NOT cross a nested lambda/closure body or a called function body. Such a body requires its own `checked` block and exposes the resulting `Res` in its callable type or return type.
6. `checked` starts a new short-circuit region for `?` authorization:
   1. a surrounding `result {}` context does not cross into the `checked` body,
   2. `?` in the body is invalid unless it is inside a nested `result {}` block,
   3. the completed `checked` expression may itself be followed by `?` when it appears inside `result {}`.
7. Calls to fallible standard-library APIs, including tensor primitives, retain their declared `Res` channels and are not intercepted or rewritten by `checked`.
8. A numeric fault captured by `checked` is a recoverable value result, not `runtime_error`, and does not make the enclosing function effectful.
9. For ownership analysis, a captured numeric-fault edge exits every active
   obligation region through the innermost `checked` body. Roots in enclosing
   regions that remain after the checked expression produces its `Err` value
   are not due on that edge.

Example:

```eglur
fn affine(a: i64, b: i64, c: i64) -> Res[i64, NumericError] =
  checked {
    let product = a * b
    yield product + c
  }

fn render(a: i64, b: i64, c: i64) -> Res[Str, NumericError] =
  result {
    let value = affine(a, b, c)?
    yield Ok(to_str(value))
  }
```


### `?` Propagation

When applied to `Res`, `?` MUST return early on `Err`.

When applied to `Opt`, `?` MUST return early on `None`.

Within `result {}`:

1. `Ok(x)?` evaluates to `x`.
2. `Err(e)?` aborts the block and returns `Err(e)`.
3. `Some(x)?` evaluates to `x`.
4. `None?` aborts the block and returns `None`.
5. For ownership analysis, an `Err`/`None` early-exit edge exits every active
   obligation region through the innermost `result` body. Roots in enclosing
   regions that remain after the result expression produces its value are not
   due on that edge.
6. `Opt[Never]?` and `Res[Never, E]?` have no normal success continuation. The
   former can only take the `None` exit; the latter can only take an `Err` exit
   when that variant is possible. Their successful payload expression type is
   `Never` and preserves the no-normal-continuation fact.
7. `Res[T, Never]?` has no possible `Err` edge and follows only its `Ok`
   continuation. These inhabitance facts do not rewrite the carrier type or
   bypass the ordinary error-channel compatibility check.

For conditional ownership roots:

1. a named carrier MUST be consumed as `(move r)?`; bare `r?` fails with `OWNERSHIP_TRANSFER_REQUIRES_MOVE`,
2. a fresh carrier expression such as `open!()?` may be consumed directly,
3. `Some`/`Ok` success establishes the payload binding as a new direct root,
4. `None`/`Err` consumes the carrier before the typed early exit and creates no direct root.

For `Res[U, Ein]?` inside a `Res[T, Eout]` result block:

1. `Ein` MUST be assignable to `Eout` under error-union assignability rules in `type-system.md`.
2. Early `Err(e)` propagation injects `e` into `Eout` without wrapping, boxing, or changing its runtime error value.
3. If the propagated error type is not assignable to the block error type, compilation MUST fail with `RETURN_TYPE_MISMATCH`.
4. If expected-type context does not fix `Eout`, the inferred error type is the normalized union of all `Res` error channels reached by `?` plus any final-yield `Err` payload constraints.


### Result Block Kind Consistency

A `result {}` block MUST evaluate to one short-circuit family in v0.1.0:

1. `Res[T, E]` if `?` is used on `Res`.
2. `Opt[T]` if `?` is used on `Opt`.

Mixing `Res`-`?` and `Opt`-`?` in the same block is disallowed and MUST be a compile-time error (`RESULT_BLOCK_MIXED_KINDS`).

Explicit conversion is allowed before `?` (for example, `opt.ok_or(err)?`).


### Result Block Final Yield Typing

The final `yield expr` in a `result {}` block yields the block's complete short-circuit value.

1. `result {}` MUST NOT implicitly wrap a yielded payload in `Ok(...)` or `Some(...)`.
2. For a `Res[T, E]` result block, the final yield expression MUST type-check as `Res[T, E]`.
3. For an `Opt[T]` result block, the final yield expression MUST type-check as `Opt[T]`.
4. If the block contains one or more `?` operations, the block family is determined by those operations under Result Block Kind Consistency above.
5. If the block contains no `?` operations, the block family and type parameters MUST be determined from expected-type context (for example an annotated binding, function return type, or surrounding expression constraint).
6. If no expected-type context determines the block family, compilation MUST fail under local type-checking rules in `type-system.md` (for example `BINDING_TYPE_UNDETERMINED`).
7. If the final yield expression is not assignable to the determined result block type, compilation MUST fail with `RETURN_TYPE_MISMATCH`.
8. An exact-`Never` final-yield expression does not determine a missing result
   family. Without `?`, Rule 5 still requires independently established expected
   `Opt[...]` or `Res[...]` context; without it Rule 6 still fails. Once that
   context is established, direct bottom compatibility applies.


### `yield` Requirement

In v0.1.0, a `result {}` block MUST end with an explicit `yield expr`.

Tail-expression implicit yield is reserved for future versions and is currently disallowed (`RESULT_MISSING_YIELD` when required and absent).

General block semantics (including function block-bodies) are defined in `control-flow.md`:

1. blocks without `yield` evaluate to `()` of type `Unit` when they reach normal
   fallthrough, and to `Never` when they have no normal result edge,
2. trailing expressions without `yield` are statement forms, not implicit block-result return.

An exact-`Never` expression does not waive the explicit-yield syntax rule.
`result { abort!(...) }` therefore remains `RESULT_MISSING_YIELD`; a written
`yield abort!(...)` satisfies the form requirement but creates no normal yield
edge under `control-flow.md`.


### Exceptions

Exceptions are outside the core failure model.

Implementations MAY provide exceptions at process boundaries, but core pipeline semantics MUST remain expressible through `Res` and `Opt`.
