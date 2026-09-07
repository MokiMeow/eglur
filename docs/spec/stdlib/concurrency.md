# `std::concurrency` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

This module defines the minimum concurrency and parallel batch API.

```eglur-catalog
opaque type Task[T]
opaque type JoinError
```

Effectful task operations:

```eglur-catalog
fn spawn![T](f: callback[task-thunk]() -> T) -> Task[T]
fn join![T](t: &Task[T]) -> Res[T, JoinError]
```

The declarations use the callback kinds defined by the
[standard-library catalog metalanguage](catalog-signatures.md).
The trailing-block form is Eglur source sugar, not a second catalog declaration:

```eglur
spawn! { body }
```

Parallel sequence operations:

```eglur-catalog
fn Seq[T].par_map![U](self: Seq[T], f: callback[effectful](T) -> U, workers: PositiveNat) -> Seq[U]
fn Seq[T].par_traverse_res![U,E](self: Seq[T], f: callback[effectful](T) -> Res[U,E], workers: PositiveNat) -> Res[Seq[U],E]
fn Seq[T].par_traverse_opt![U](self: Seq[T], f: callback[effectful](T) -> Opt[U], workers: PositiveNat) -> Opt[Seq[U]]
```

Normative semantics:

1. `spawn!` MUST be bang.
2. Direct `spawn!({ || body })` calls, `spawn! { body }` sugar, and
   preconstructed pure `Fn() -> T` values all satisfy the
   `callback[task-thunk]` parameter above:
   1. an inline thunk body is an effectful child task body and MAY call bang APIs such as `emit!` or `abort!`,
   2. the sugar form is valid only for resolved `std::concurrency::spawn!` calls,
   3. `spawn! { body }` is equivalent to `spawn!({ || body })` for type checking and result typing,
   4. the thunk body uses closure block tail-expression semantics: a final
      expression, when present, is the child task result; no final expression
      yields `()` of type `Unit` on normal fallthrough, while a body with no
      normal continuation has result type `Never`,
   5. the enclosing `spawn!` call itself still requires a bang caller,
   6. a preconstructed source value MUST have static type `Fn() -> T` and
      therefore remains pure,
   7. `spawn!` schedules invocation of that existing value without reevaluating
      or replacing its closure environment,
   8. an incompatible preconstructed callable arity or result type fails with
      `CALL_ARG_TYPE_MISMATCH`.
3. `join!` MUST borrow the task value without consuming or mutating that value and return either `Ok(value)` or `Err(JoinError)`.
   Its wait is a completion-wait boundary and MUST satisfy the boundary-local
   progress rule in `../language/concurrency.md`.
4. `Task[T]` is an ownership-free immutable completion value in v0.1:
   1. `T` MUST be ownership-free and reference-free,
   2. assignment, argument passing, and return copy the same computation identity under ordinary value semantics,
   3. copying a task value MUST NOT spawn, restart, detach, cancel, or otherwise duplicate child execution,
   4. `Task[T]` does not implement `Eq`; task equality, ordering, and hashing are not part of the v0.1 baseline.
5. `join!` MAY be called repeatedly through one task value or any of its copies.
   Every call MUST return the same value-semantic completion result; implementations MAY share or copy memoized storage when ordinary `T` value semantics remain observable.
6. If the joined child task terminates through any `runtime_error` outcome,
   including `RUNTIME_ABORT` or a runtime `EXTERN_LINKAGE_FAILED`, `join!` MUST
   return `Err(JoinError)` and MUST NOT itself terminate the runtime. The child's
   cause code is not re-emitted at the join boundary.
7. An explicit `join!` call that returns the child's `Err(JoinError)` marks that child failure observed for the spawning scope.
   Joining any task-value copy has the same observation effect.
8. v0.1 baseline does not require a stable public variant set or payload schema for `JoinError`; only the repeatable `Err(JoinError)` branch is baseline-observable.
9. Every child task spawned in the current bang function/task body MUST complete before control returns to the caller.
   Scope exit waits for children that are still running under the same
   completion-wait progress rule but MUST NOT consume or invalidate their task
   values.
10. A task value MAY escape its spawning body, but its child still completes before that body returns; later `join!` calls therefore observe the memoized completed result.
11. If a child reaches `Err(JoinError)` from a `runtime_error` outcome and no
   explicit `join!` has observed that failure before scope exit completes,
   runtime MUST terminate through runtime-abort channel with code
   `RUNTIME_ABORT` and `data.source` equal to exact string
   `std::concurrency::scope_exit_join`. This scope-exit projection takes
   precedence over re-emitting the child's inner cause code.
   1. Fatal diagnostic help SHOULD direct users toward explicit `join!` when recoverable child-failure handling is desired.
   2. Fatal diagnostic related text SHOULD identify the scope-exit child-wait boundary.
12. `par_*` APIs MUST be bang.
13. The `workers` argument to every `par_*` API is a `PositiveNat`.
14. Signed runtime worker-count values MUST be converted with `PositiveNat.from_i64` before use; failed conversion uses `DOMAIN_INVALID_VALUE`.
15. A zero or negative worker count MUST NOT reach callback scheduling through the typed public API boundary.
16. The typed public API has no runtime-abort invalid-worker path; worker validation occurs at the `PositiveNat` boundary.
17. `workers` is a maximum worker-budget request, not a guaranteed parallelism level; implementations MAY still execute sequentially or with fewer workers if the observable result/index contracts below are preserved.
18. `par_map!` MUST preserve input order in output sequence.
19. `par_traverse_res!` MUST return the `Err` produced by the earliest failing 0-based input index.
20. `par_traverse_res!` diagnostics MUST use `TRAVERSE_RES_FAIL` and report that same earliest failing 0-based input index.
21. `par_traverse_opt!` MUST return `None` if any input returns `None`.
22. If `par_traverse_opt!` emits diagnostics for `None`, they MUST use `TRAVERSE_OPT_NONE` and report the earliest `None` 0-based input index.
23. For `par_traverse_res!` and `par_traverse_opt!`, earliest-index semantics are defined by minimal input index among failing/`None` elements, not by first completion order.
24. `par_*` callback start/completion/effect interleaving order inside the call boundary is implementation-defined, subject to the fail-fast return boundary below.
25. `par_*` implementations MAY execute sequentially, concurrently, or by mixed batching and remain conforming if the observable result/index contracts above are preserved.
26. `par_*` fail-fast result/index semantics do not imply rollback or suppression of already-started later callbacks.
27. Sequential traversal callback-order rules in `../language/seq-traverse.md` do not apply to these `par_*` APIs.
28. v0.1 baseline does not require preemption or minimum parallel speedup.
    Outside an active completion-wait boundary, it also does not require
    fairness, starvation-freedom, or eventual-start guarantees.
29. `Task[T]` is an opaque runtime-sized completion value for recursive-layout analysis; `T` is not stored inline in the value.

## Fail-Fast Parallel Traversal Return Boundary

`par_traverse_res!` and `par_traverse_opt!` are quiescent value-return
boundaries. The following rules define when fail-fast selection becomes
decisive and what work may cross the return boundary:

1. For `par_traverse_res!`, input index `k` is **decisive** once callback `k`
   has returned `Err(error)` and every callback at an input index lower than
   `k` has returned `Ok(_)`.
2. For `par_traverse_opt!`, input index `k` is **decisive** once callback `k`
   has returned `None` and every callback at an input index lower than `k` has
   returned `Some(_)`.
3. A callback invocation is **begun** once control has entered its callback
   body. A queued or otherwise selected invocation whose body has not been
   entered remains unstarted.
4. Once a decisive index exists, an unstarted callback at any greater input
   index MUST NOT begin.
5. Every callback invocation begun before the decisive index became known MUST
   complete through its ordinary value channel before the `par_traverse_*`
   operation returns a typed value.
   1. An `Err` or `None` from a greater input index does not replace the result
      selected by the decisive index.
   2. Effects already performed, including effects from greater input indices,
      are not rolled back.
   3. Any structured child wait or scoped cleanup required for the callback to
      return is part of callback completion.
6. Consequently, after a `par_traverse_*` operation returns a typed value, no
   effect, child wait, or scoped cleanup from any callback begun by that call
   remains pending.
7. If any begun callback reaches a `runtime_error` outcome before typed return,
   the operation follows that runtime-error channel and MUST NOT return the
   otherwise selected `Ok`, `Err`, `Some`, or `None` value.
8. A begun callback that blocks or does not terminate MAY prevent the operation
   from returning indefinitely. v0.1 provides no cancellation or timeout
   outcome for this boundary.

## Coordination Primitive Boundary

v0.1 baseline conformance intentionally excludes coordination primitives such as channels, mutexes, and atomics.

1. Baseline v0.1 conformance MUST NOT require channel APIs (`send!`, `recv!`, close semantics, buffering profiles).
2. Baseline v0.1 conformance MUST NOT require mutex/lock APIs.
3. Baseline v0.1 conformance MUST NOT require atomic memory-operation APIs.
4. Implementations MAY expose these surfaces as non-baseline extensions in v0.1.
5. If provided as extensions, blocking or scheduler-interacting operations MUST remain bang-named.
6. Follow-up coordination design is tracked by `STDLIB-OI-020` in `../open-items/stdlib.md`.
