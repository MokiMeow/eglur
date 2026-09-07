# Concurrency and Parallelism

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Task Model

`Task[T]` is the core asynchronous completion value.

`T` is ownership-free and reference-free in v0.1. A task value identifies one spawned computation and
its memoized completion result; copying, rebinding, passing, or returning the task
value does not spawn another computation.

Scheduling and runtime strategy are implementation-defined unless constrained below.

### Task Thunk Capture

The trailing `spawn! { ... }` task body and an inline zero-parameter closure
passed directly to `spawn!` construct the task thunk during call-argument
evaluation.
A preconstructed pure `Fn() -> T` value may also be used as a task thunk.

Rules:

1. Free variables in an inline task-thunk closure are resolved and validated
   under the closure capture contract in `calls-references.md`.
2. Ordinary immutable values captured by an inline task thunk are snapshotted
   when its closure expression is evaluated as part of the `spawn!` call,
   before the child may begin executing.
3. A preconstructed `Fn() -> T` retains the closure environment fixed when that
   value was constructed.
   `spawn!` MUST NOT reevaluate its capture expressions, reopen its defining
   lexical scope, or replace that environment with a new snapshot.
4. The task thunk is escaping for reference-capture analysis because execution
   may outlive the invocation expression:
   1. capturing an explicit reference fails with `REF_ESCAPE_FORBIDDEN`,
   2. capturing an ownership root or a reference derived from one fails with
      `OWNERSHIP_TYPE_POSITION_FORBIDDEN`.
5. Capturing a `let mut` binding or writing storage rooted in a captured
   binding fails with `MUTABLE_CAPTURE_FORBIDDEN`.
6. Inline task-body effect authorization does not relax Rules 1-5 and does not
   create a first-class effectful callable type.
   A preconstructed source `Fn() -> T` remains pure.
7. Task scheduling and start timing MUST NOT change which closure environment
   is invoked.

### Observable Scheduling Boundary

v0.1 baseline constrains concurrency only at observable result/lifetime boundaries.

1. `spawn!` completion-value creation, `join!` result-channel behavior, and scope-exit child wait are normative.
2. Task start timing is implementation-defined:
   1. `spawn!` MAY begin execution eagerly,
   2. `spawn!` MAY defer execution until a later runtime scheduling point,
   3. task start timing by itself MUST NOT be used as a conformance criterion.
3. Task interleaving, callback start order, and callback completion order are
   implementation-defined.
4. Outside an active completion-wait boundary defined below, fairness,
   starvation-freedom, and eventual-start guarantees are not part of v0.1
   baseline.
5. Required baseline suites MUST NOT require actual parallel speedup or a minimum degree of simultaneous execution.
6. `join!` borrows a task value and MAY be called repeatedly through the original value or any copy.
7. Every successful or failed repeated `join!` of one task MUST return the same value-semantic completion result.
8. `Task[T]` does not implement `Eq`; task identity, equality, ordering, and hashing are not baseline-observable.


### Completion-Wait Progress

An explicit `join!` wait and the implicit child wait at bang function/task-body
exit are completion-wait boundaries.

A task is runnable for this section when the runtime can select it for continued
execution without first waiting for an external event or another task's
completion. Runnable state is runtime scheduling state, not a source-level type
or annotation.

Rules:

1. A task is wait-required while its completion is transitively necessary for
   the active boundary to return:
   1. an explicit `join!` makes the joined task wait-required,
   2. scope exit makes every unfinished child spawned in that body
      wait-required,
   3. structured descendants that must complete before a wait-required task can
      complete are also wait-required.
2. On entering the boundary, and whenever the runtime regains control there,
   the runtime MUST create or enable a scheduling opportunity when any
   wait-required task is runnable. It MUST NOT passively park all available
   execution capacity while runnable wait-required work remains deferred.
3. Across any unbounded sequence of runtime scheduling opportunities while the
   boundary remains active, every continuously runnable wait-required task MUST
   eventually be selected for execution.
4. The runtime MUST NOT indefinitely select unrelated work, or one repeatedly
   yielding wait-required task, while another wait-required task remains
   runnable.
5. Implementations MAY run wait-required work inline on the waiting thread,
   enqueue or unpark it on another worker, use work stealing, or combine these
   strategies. The same observable rule applies to single-threaded and
   multithreaded runtimes.
6. Boundary-local progress does not require a fixed scheduling order, parallel
   execution, preemption, a wall-clock completion bound, minimum throughput, or
   fairness outside the active boundary.
7. Implementations MUST NOT attempt to decide whether a task is finite as a
   prerequisite for applying Rules 2-5.
8. A completion-wait boundary MAY wait indefinitely when a wait-required task:
   1. is blocked on an event that never occurs, or
   2. is selected for execution as required but diverges, whether or not the
      runtime later regains scheduling opportunities.
9. Indefinite waiting under Rule 8 is not by itself a language-level value,
   diagnostic, runtime error, or timeout result.


### Structured Concurrency

Spawned tasks MUST be scoped to structured lifetime boundaries.

v0.1 baseline fixes scope-exit behavior to a structured child wait:

1. The required boundary is function/task lexical exit.
2. A **runtime-fatal child completion** is a child outcome on the
   `runtime_error` channel, including `RUNTIME_ABORT` and a runtime
   `EXTERN_LINKAGE_FAILED`. If such a child is explicitly joined, `join!` MUST
   surface that failure as `Err(JoinError)`, mark that child failure observed for
   the spawning scope, and MUST NOT itself terminate the runtime. The child's
   cause code is not re-emitted at the join boundary.
3. v0.1 baseline does not require a stable public variant set or payload schema for `JoinError`.
4. On exit from a bang function/task body, implementations MUST wait for every child task spawned in that body before returning control, including children already observed through explicit `join!`, under the completion-wait progress rule above.
5. Scope exit MUST NOT consume, invalidate, or change the repeatable result of any task value.
6. An explicit `join!` marks the corresponding child completion observed for the spawning scope; joining any copy has the same observation effect.
7. If any child has a runtime-fatal completion and no explicit `join!` has
   returned that child's `Err(JoinError)` before scope exit, execution MUST
   terminate through runtime-abort channel with code `RUNTIME_ABORT` and
   `data.source` equal to exact string `std::concurrency::scope_exit_join`.
   This scope-exit projection takes precedence over re-emitting the child's
   inner cause code.
8. A task value MAY escape its spawning body, but the underlying child MUST still complete before that body exits; the escaped value therefore denotes an already-completed result after return.


### Parallel Sequence Operations

`Seq.par_*` operations are part of the required standard module surface (`std::concurrency`).

At minimum, implementations MUST provide:

1. `par_map!`
2. `par_traverse_res!`
3. `par_traverse_opt!`

Default output ordering and failure-index behavior MUST be deterministic where specified by the standard-library semantics.

Implementations MAY realize `par_*` operations concurrently, sequentially, or by mixed batching as long as the documented observable result/index contracts are preserved.

Callback start/completion/effect interleaving order for `par_*` operations is implementation-defined and MUST NOT be used as a conformance criterion except for an explicitly controlled scheduler probe.

Each fail-fast `par_traverse_*` operation is a quiescent value-return boundary.
Once the minimal failing/`None` input index is decisive, no unstarted later
callback may begin, and every callback already begun must complete before the
operation returns a typed value. Implementation-defined interleaving therefore
does not permit callback effects or cleanup to continue after typed return. The
canonical decisive-index, runtime-error precedence, and nontermination rules are
defined by [`std::concurrency`](../stdlib/concurrency.md#fail-fast-parallel-traversal-return-boundary).

The `workers` argument to each `par_*` operation is `PositiveNat`.
Zero or negative signed worker counts MUST fail before callback scheduling at the explicit `PositiveNat` conversion or contextual literal boundary (`DOMAIN_INVALID_VALUE`).
