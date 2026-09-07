# Global Conventions

Scope: Normative for v0.1 unless this file or a section states otherwise.

Normative standard-library declarations use the closed
[catalog signature metalanguage](catalog-signatures.md).

### Effects and Bang Naming

Any operation that may perform an effect MUST be named with a trailing `!`.

Effects include:

1. I/O: file, network, console, process, IPC.
2. State mutation: mutation of externally observable state.
3. Non-determinism: time, randomness, environment, scheduler-dependent behavior.
4. Concurrency runtime actions: spawning tasks, sleeping, scheduler interaction.

Therefore the standard library:

1. MUST name I/O and nondeterministic functions with `!`.
2. MUST name mutating operations with `!`.
3. MUST NOT expose mutating operations without `!`.

Examples: `print!`, `read_text!`, `write_text!`, `now!`, `rand!`, `spawn!`, `sleep!`, `set!`, `sort!`.


### Pure vs Effectful Higher-order APIs

To avoid hidden effects, higher-order APIs are either pure-only adapters or explicit pure/effectful split families.

1. Pure-only adapters (non-bang) accept only pure callbacks and have no required bang sibling in v0.1.
2. Pure/effectful split families expose a non-bang form that accepts only pure callbacks and a bang form that MAY accept callbacks invoking bang functions.
3. Any non-bang higher-order API that receives an effectful callback MUST fail with `PURE_COMBINATOR_EFFECTFUL_CALLBACK`.
4. A bang API MAY still require a pure callback when its bang marker exposes an independent effect such as receiver mutation. An effectful callback passed to that pure callback parameter MUST fail with `PURE_COMBINATOR_EFFECTFUL_CALLBACK`.

At minimum, this split is REQUIRED for:

1. `map` vs `map!`.
2. `tap` vs `tap!`.
3. `traverse_res` vs `traverse_res!`.
4. `traverse_opt` vs `traverse_opt!`.

`std::opt` and `std::res` branch-adapter APIs (`map`, `and_then`, `map_err`, `or_else`, `unwrap_or_else`, `ok_or_else`) are pure-only adapters in v0.1; bang siblings for those adapters are not part of the required baseline.

`for_each!` is always effectful.

v0.1 defines no effectful callable type. Core type and syntax work is tracked by
`LANG-OI-010` in `../open-items/language.md`; adoption by standard-library
callback APIs is tracked by `STDLIB-OI-002` in `../open-items/stdlib.md`.


### Ownership-producing Resource Acquisition

This convention applies only to ownership-producing acquisition operations whose
module contract explicitly invokes it.

1. `Ok(Own[R])` establishes exactly one source-level resource root. That root
   remains subject to the transfer or consuming-close obligation for `Own[R]`.
2. `Err(E)` establishes no new source-level root and MUST NOT carry a recoverable
   or hidden `Own[R]` value.
3. If an implementation resource was acquired before the failure became known,
   the implementation MUST complete the target-specific terminal-release
   protocol before returning `Err`. It MUST retain no usable handle or reference
   for that provisional resource and MUST NOT schedule implementation-managed
   hidden later cleanup.
4. Terminal retirement of a provisional resource does not roll back external
   effects that preceded the failure. An `Err` MAY therefore follow externally
   observable filesystem, process-stream, or transport progress when the module
   contract permits it.
5. Unless a module specifies a stronger result, the error does not identify the
   failing lifecycle phase or prove that no external effect occurred.
6. The compiler, runtime, and standard library MUST NOT automatically replay a
   failed acquisition. A caller-initiated retry is a fresh acquisition against
   the then-current path, process stream, listener queue, or network state and is
   not generally idempotent.


### Scoped Resource Callbacks

Bang APIs may expose a scoped resource callback directly in their catalog contract.
Such a parameter is declared as
`f: callback[scoped-resource](&mut Own[R]) -> Res[T, E]`; this catalog-only
callback kind is not the pure source callable type `Fn(&mut Own[R]) -> Res[T, E]`.
The callback body belongs to caller-supplied code, while acquisition and close
belong to the scoped API. If the callback and close both fail, `ScopedFailure`
preserves both errors.

Rules:

1. A scoped resource callback MAY call bang APIs.
2. The callback MUST be invoked exactly once after successful acquisition and MUST NOT be invoked when acquisition fails. An ownership-producing acquisition follows the shared acquisition convention when its module invokes that convention.
3. The callback receives one non-escaping writable reference to the internal resource root.
4. The callback MUST NOT store, return, or capture that reference in another closure or value.
   Nested scoped operations therefore MUST NOT capture an outer resource root or a reference derived from one.
5. `T` and `E` are ownership-free and reference-free in v0.1.
6. The scoped API MUST invoke its consuming close operation exactly once after every typed callback exit, including `Err` and `?` exits. That source-level invocation follows the terminal consuming-close convention when its module invokes that convention.
7. A scoped API with callback error type `E` returns
   `Res[T, ScopedFailure[E]]`. Its complete typed outcome table is:

   | Acquisition | Callback | Close | Result |
   | --- | --- | --- | --- |
   | `Err(acquisition)` | not invoked | not invoked | `Err(ScopedFailure::Primary(acquisition))` |
   | `Ok(root)` | `Ok(value)` | `Ok(())` | `Ok(value)` |
   | `Ok(root)` | `Ok(_)` | `Err(close)` | `Err(ScopedFailure::Primary(close))` |
   | `Ok(root)` | `Err(primary)` | `Ok(())` | `Err(ScopedFailure::Primary(primary))` |
   | `Ok(root)` | `Err(primary)` | `Err(close)` | `Err(ScopedFailure::PrimaryAndClose { primary, close })` |

8. `Primary` deliberately does not identify whether its error came from
   acquisition, callback execution, or close. The phase is observable only when
   `PrimaryAndClose` proves that the callback and close both failed.
9. `PrimaryAndClose.primary` is the callback's exact `E` value and remains the
   primary failure. `PrimaryAndClose.close` is the exact later close `E` value.
   Neither value may be replaced, collapsed, logged out of band, or discarded.
10. `E` and `ScopedFailure[E]` are distinct types. This convention adds no
    implicit conversion or special `?` rule between them. A surrounding
    function that propagates both ordinary `E` failures and scoped failures may
    declare the existing error union `Union[E, ScopedFailure[E]]`.
11. A nested scoped operation that does not capture the outer root is allowed,
    but its `ScopedFailure` result MUST be handled at that inner call boundary;
    scoped APIs do not implicitly flatten nested failure carriers.
12. Fatal runtime termination is not a typed callback exit. It MUST NOT be
    converted to `ScopedFailure`, and v0.1 makes no typed close guarantee on that
    path.
13. Direct callback arguments and trailing-closure sugar denote the same contract.
14. This convention does not define a general first-class effectful callable type; that remains governed by the language open-item boundary.


### Library-Owned Managed Resource Operations

This convention applies only when a module explicitly invokes it for a
convenience operation that owns acquisition, a fixed library-defined body, and
mandatory close. It does not apply to an operation with a
`callback[scoped-resource]` parameter.

Rules:

1. A library-owned managed resource operation with error type `E` returns
   `Res[T, E]`; internal lifecycle phases alone do not introduce
   `ScopedFailure[E]`.
2. After successful acquisition, the operation MUST invoke its consuming close
   exactly once after either typed body result. The close follows the terminal
   consuming-close convention when the module invokes that convention.
3. Its complete typed outcome table is:

   | Acquisition | Body | Close | Result |
   | --- | --- | --- | --- |
   | `Err(acquisition)` | not invoked | not invoked | `Err(acquisition)` |
   | `Ok(root)` | `Ok(value)` | `Ok(())` | `Ok(value)` |
   | `Ok(root)` | `Ok(_)` | `Err(close)` | `Err(close)` |
   | `Ok(root)` | `Err(primary)` | `Ok(())` | `Err(primary)` |
   | `Ok(root)` | `Err(primary)` | `Err(_)` | `Err(primary)` |

4. In the final row, the later close failure is not represented separately in
   the source-level result. This does not establish that close succeeded or
   weaken terminal-retirement obligations.
5. This convention treats acquisition, body, and close as phases of one
   library-owned public operation. It MUST NOT be used to collapse a
   caller-owned scoped callback failure with an API-owned close failure.
6. Fatal runtime termination is not a typed body exit and remains outside typed
   cleanup guarantees unless a module defines a stronger contract.


### Ordered Byte-Chunk Reader Calls

This convention applies only to `read_chunk!` operations whose module contract
explicitly invokes it. It governs logical byte delivery and error progress for
one borrowed sequential reader; acquisition, close, random access, and
module-specific EOF classification have their own contracts.

1. Source bytes form one ordered stream. A byte is logically delivered only
   when it appears in an `Ok(Some(chunk))` result; physical acquisition into an
   implementation-private buffer is not delivery.
2. Across successful calls, errors, and retries, returned chunks MUST preserve
   source order and MUST NOT deliver any source byte more than once.
3. `Ok(None)` MUST NOT be returned while any source byte ordered before EOF
   remains buffered but undelivered.
4. An outward `Err(E)` has zero logical byte progress for that call. The
   implementation MUST NOT discard a source byte that it acquired but has not
   delivered.
5. If an underlying read reports bytes together with a failure, or a failure is
   discovered after bytes have been prefetched, every acquired byte ordered
   before that failure MUST be retained and delivered through one or more
   `Ok(Some(chunk))` results before the failure is surfaced as `Err(E)`.
6. A later call after `Err` is a new read attempt. It MUST NOT replay bytes
   delivered by an earlier call and, if it succeeds, MUST resume at the next
   undelivered source byte. This convention does not guarantee that the source
   remains viable or that a later call succeeds.
7. For a method borrowing `&mut Own[R]`, `Err` neither discharges nor statically
   invalidates the root. Later method calls remain type-valid, and the root
   remains subject to its transfer or consuming-close obligation.


### Payload-Completing Writer Calls

This convention applies only to operations whose module contract explicitly
invokes it. It governs progress for one payload-completing write; acquisition,
flush, close, and whole-file replacement or durability have their own
contracts.

1. `Ok(())` MUST be returned only after every byte of the call's payload has
   been accepted by the operation's writer.
2. Acceptance means transfer into the writer's ordered output state. Unless an
   operation specifies a stronger contract, acceptance alone does not establish
   external visibility, durable storage, or peer receipt.
3. `Err(E)` MAY be returned after the writer has accepted any ordered prefix of
   the payload, including the empty prefix or the full payload. The error carries
   no progress count; callers MUST NOT infer that zero bytes or all bytes were
   accepted.
4. An accepted or externally visible prefix is not rolled back. Any bytes from
   this payload made externally visible MUST preserve source order and form a
   prefix, but buffering MAY make the visible prefix shorter than the accepted
   prefix.
5. After returning `Err`, the standard library and runtime MUST NOT replay the
   payload as an implicit recovery action. A later caller invocation is a new
   whole-payload write, not continuation from an inferred suffix, and MAY
   duplicate a prefix. Retry safety exists only when an independent application
   protocol provides it.
6. For a method borrowing `&mut Own[R]`, `Err` neither discharges nor statically
   invalidates the root. Later method calls remain type-valid, but this
   convention provides no guarantee about protocol state or later write
   success. The root remains subject to its transfer or consuming-close
   obligation.


### Ordered Writer Flush

This convention applies only to `flush!` operations whose module contract
explicitly invokes it. It governs bytes already accepted into one writer's
ordered output state; it does not accept a new payload.

1. `Ok(())` MUST be returned only after every byte accepted before the call has
   been transferred from the Eglur writer's intermediate buffer to its
   underlying writer, in order.
2. `Err(E)` MAY be returned after transfer of any ordered prefix of the pending
   bytes, including the empty prefix or all pending bytes. The error carries no
   transferred-byte count or lifecycle-phase discriminator.
3. The implementation MUST retain exactly the not-yet-transferred suffix in the
   writer's ordered output state. It MUST NOT replay bytes already transferred
   by the failed call.
4. The borrowed writer root remains owned and type-valid after `Err`. Any later
   accepted payload is ordered after all bytes accepted before the failed flush.
5. A later `flush!` or consuming close attempt MUST continue from the retained
   suffix. A successful later flush or close completes transfer of the pending
   bytes without duplicating the transferred prefix. This convention does not
   guarantee that a later attempt succeeds.
6. Transfer to the underlying writer does not establish `fsync`-style stable
   storage, peer receipt, terminal processing, or survival across process,
   kernel, device, network, or power failure.


### Terminal Consuming Close

This convention applies only to consuming `close!` operations whose module
contract explicitly invokes it.

1. One source-level `(move root).close!()` invocation consumes and discharges the
   root before either `Ok` or `Err` becomes observable. The root cannot be
   returned, retried, inspected, or used for later I/O.
2. Before returning either result, the implementation MUST complete one
   target-specific logical terminal-release protocol. It MUST retain no usable
   handle or reference corresponding to the consumed root and MUST NOT schedule
   implementation-managed hidden later cleanup for that resource.
3. One logical terminal release is not necessarily one host call. A platform
   adapter MAY repeat or vary host operations only when the target's documented
   resource semantics make that safe and necessary, and MUST NOT retry a host
   operation when doing so could act on a reused or different resource.
4. An implementation target that cannot establish terminal retirement MUST NOT
   claim conformance for the affected resource surface.
5. Closing a writer MUST attempt transfer of its pending ordered output before
   terminal release and MUST continue to terminal release even when that
   transfer fails:
   1. `Ok(())` requires complete transfer of the pending output and terminal
      local release,
   2. `Err(E)` MAY follow transfer of any pending prefix, including all pending
      bytes, but MUST NOT leave a source-owned or hidden local resource,
   3. neither result strengthens the ordered-flush durability or peer-observation
      boundary.
6. Closing a reader performs terminal local release without an output-progress
   guarantee.
7. Unless a module specifies a stronger error surface, `Err` does not identify
   whether pending transfer or release failed. The compiler, runtime, and
   standard library MUST NOT automatically replay output or retry the consumed
   source-level close.


### Determinism

Unless explicitly specified otherwise, iteration order MUST be deterministic.

Implementations MUST expose determinism execution profiles for numeric tensor execution:

1. `default` profile.
2. `strict` profile request surface.

The v0.1 source-level request surface is typed, not stringly typed:

1. `std::tensor::DeterminismProfile` has exactly `DeterminismProfile::Default` and `DeterminismProfile::Strict` in the required baseline.
2. Tensor APIs that expose a per-call determinism request MUST name the parameter `profile` and type it as `DeterminismProfile`.
3. String values such as `"default"` or `"strict"` MUST NOT be accepted for `profile`.
4. In v0.1, `std::tensor::index::set_at` and `std::tensor::index::set_at!` are the only required per-call profile-bearing tensor APIs.
5. Tensor APIs without an explicit `profile` parameter execute under the implementation's active default determinism context; v0.1 does not define an ambient profile setter or attribute-style determinism surface.

Strict profile support MAY depend on backend/device capabilities. If strict is
requested through a profile-bearing API but unavailable, execution MUST fail
with `DETERMINISM_UNAVAILABLE` and MUST NOT silently fall back to `default`.
An implementation MUST NOT activate `strict` as the context for profile-less
APIs unless every required primitive in that execution context has a certified
strict path. Failure to establish such a context is an implementation
activation/certification failure outside source execution; it does not add
`DeterminismError` to profile-less API signatures.

#### Primitive-level Contract

Determinism policy is defined at Core Tensor IR primitive level.

High-level operations (`reduce`, `contract`, `einop`, `map_axis`, `grad`, indexing/scatter APIs) inherit these guarantees through lowering.

The following identities define the numeric repeatability boundary:

1. A **compiled artifact identity** is a content address of the immutable
   executable artifact actually evaluated, including statically selected
   lowering and code-generation choices.
2. An **execution plan identity** is a content address of the complete selected
   plan for that evaluation. It MUST cover every output-relevant kernel,
   fusion, scheduling, math-mode, and dynamic-dispatch choice not already fixed
   by the compiled artifact.
3. A **fixed execution identity** is the tuple of compiled artifact identity,
   execution plan identity, compiler and runtime identities/revisions, target,
   determinism profile, backend, device, driver/runtime,
   worker/thread/process configuration, and randomness configuration.
4. Output-relevant tuning MUST complete before an activated artifact instance's
   first observed value evaluation. The selected plan identity then remains
   fixed for that instance.
5. An execution plan MAY contain deterministic dispatch over exact input facts
   such as shape or dtype. The complete dispatch table is part of the plan
   identity, and identical inputs MUST select the same entry.
6. Timing feedback, scheduler state, or a mutable cache MUST NOT select a new
   output-relevant plan between observed evaluations in one activated artifact
   instance. A new plan requires a new fixed execution identity outside that
   instance's evaluation sequence.
7. These identities are implementation and conformance evidence. v0.1 does not
   add a source-level API for reading or selecting artifact or plan identities.

#### Lowering-Invariant Observability

For tensor execution, implementation details of decomposition/planning/fusion are not normative.

Implementations MAY choose different valid lowerings (including different contraction plans, fusion groupings, and primitive execution schedules) as long as all of the following hold:

1. profile-specific value criteria (`default` numeric criteria or `strict` bitwise criteria) are satisfied,
2. NaN/Inf/signed-zero rules are satisfied,
3. error behavior remains profile-invariant as defined below.

Except where an operation contract fixes logical order because that order changes
observable values or failures, primitive execution order is implementation-defined
and MUST NOT be used as a conformance criterion by itself. The checked integer
reduction order below is one such fixed contract.

Lowering-independent validation failures (for example shape, dtype, axis, dim-solve, and strict-profile availability) MUST map to the same diagnostic class regardless of chosen lowering.

#### Default Profile Requirements

For all primitives in `default`:

1. Repeated evaluations of the same normalized operation with bitwise-identical
   initial inputs and identical fixed execution identity MUST produce:
   1. bitwise-equivalent tensor result values,
   2. bitwise-equivalent mutable tensor post-state where applicable,
   3. the same error/non-error outcome, primary error code, required diagnostic
      data, and fail-fast boundary index where one is defined.
   This requirement includes exact floating-point payload bits and is not
   weakened by the cross-identity tolerance below.
2. In every execution, a primitive's non-floating numeric and non-numeric
   outputs MUST match deterministic baseline exactly for its actual
   bitwise inputs.
3. In every execution, floating-point outputs MUST be numerically equivalent to
   deterministic baseline under dtype-specific tolerances:
   1. `f16`/`bf16`: `rtol=1e-2`, `atol=1e-3`.
   2. `f32`: `rtol=1e-6`, `atol=1e-8`.
   3. `f64`: `rtol=1e-12`, `atol=1e-15`.
   4. `complex64`: apply `f32` tolerances independently to real and imaginary parts.
   5. `complex128`: apply `f64` tolerances independently to real and imaginary parts.
4. Floating comparison in `default` MUST use:
   1. `abs(x - ref) <= atol + rtol * abs(ref)` for real values.
   2. For complex values, apply the same inequality independently to real and imaginary components.
5. NaN/Inf classification MUST match deterministic baseline exactly:
   1. NaN placement and count MUST match.
   2. Infinity sign placement MUST match.
6. Signed-zero classification (`+0.0` vs `-0.0`) MUST match deterministic baseline exactly.
7. Error behavior MUST remain invariant across valid fixed execution identities
   and between `default` and supported `strict` execution:
   1. same error/non-error outcome for identical logical inputs;
   2. same primary error code and required diagnostic data;
   3. same fail-fast boundary index where boundary index is defined by operation semantics.
   Strict-profile rejection with `DETERMINISM_UNAVAILABLE` is the sole profile
   availability exception to this rule.

Rules 2-6 apply to every execution, not only when two identities are compared.
Distinct fixed execution identities may expose different floating/complex bits
only when every result independently satisfies Rules 3-6.

The tolerance in Rules 3 and 4 does not redefine scalar or tensor equality or
make bit differences unobservable. Exact observers such as `bitcast`, total
equality/order/hash, checked casts, and serialization operate on the actual
input bits and remain exact for bitwise-identical inputs. Their downstream
non-floating results MAY differ across fixed execution identities only as the
deterministic consequence of permitted upstream floating/complex bit
variation; they MUST NOT introduce independent variation, and error behavior
remains constrained by Rule 7. No such variation is permitted within one fixed
execution identity. When an exact observer is fallible and permitted upstream
variation could change its error outcome, the implementation MUST use a
bit-stable upstream path or an equivalent strategy that preserves Rule 7.

#### Strict Profile Requirements

The strict baseline is context-bound unless an operation contract fixes a
cross-context canonical order or bit pattern. It is defined by the immutable
compiler/runtime/backend/device/driver/target context and its certified strict
math implementation; compiled artifact and execution plan choices MUST NOT
change it. For scatter and other operations with an explicit canonical logical
order, that operation-defined order is the strict baseline. v0.1 does not claim
one cross-backend bit algorithm for transcendental functions.

In every supported `strict` execution for identical logical inputs and strict
reference context:

1. Primitive result values and mutable tensor post-state MUST be
   bitwise-equivalent to the applicable strict baseline.
2. Error behavior MUST match `default` profile error behavior constraints above.

Determinism context at minimum includes:

1. compiled artifact identity,
2. execution plan identity,
3. compiler build/version,
4. runtime build/version,
5. target identity,
6. backend identifier,
7. device model,
8. driver/runtime version,
9. determinism profile,
10. worker/thread/process configuration,
11. randomness configuration.

#### Backend Certification and Optimization

1. A conforming implementation MUST execute every required default-profile
   primitive through a path certified to satisfy fixed-identity repeatability.
   It MAY use an audited backend kernel or a repeatable fallback.
2. An implementation MUST NOT execute an uncertified kernel and classify its
   within-identity variation as default-profile numeric tolerance. Lack of a
   certified required path is an implementation-conformance failure; it does
   not add `DeterminismError` to APIs whose catalog signature omits it.
3. Strict-profile unavailability remains the explicit
   `DETERMINISM_UNAVAILABLE` value-channel failure for profile-bearing APIs.
   A profile-less active strict context instead follows the activation rule
   above.
4. Common-subexpression elimination and memoized result reuse are valid only
   when they preserve the fixed execution identity contract. Cross-identity
   reuse MUST either prove bitwise equivalence or include every
   output-relevant identity component in the cache boundary.
5. Autotuning and plan caches MAY choose a plan before an artifact instance is
   activated for observed evaluation. Any output-relevant choice MUST be
   represented by the execution plan identity. After activation, cache
   mutation, timing feedback, or scheduler choice MUST NOT change the plan or
   evaluated result bits.


### Numeric Promotion Lattice (Scalar and Tensor)

These promotion rules are normative for binary numeric arithmetic in both scalar expressions and tensor primitives (`add`, `sub`, `mul`, `div`, `rem`, `max`, `min`, `where` branch merge, and contraction multiply/sum paths), except where `/.` defines a dedicated fractional-division carrier rule below.
Integer literals are contextually typed or defaulted before `Promote(A,B)` is
applied under `../language/type-system.md`; promotion MUST NOT search for a
fitting literal dtype or revise the completed literal type.

Domain precedence:

1. `bool`
2. integer
3. floating
4. complex

Within-domain precedence:

1. integer: `u8 < i8 < u16 < i16 < u32 < i32 < u64 < i64`
2. floating: `f16 < bf16 < f32 < f64`
3. complex: `complex64 < complex128`

Binary promotion function `Promote(A,B)`:

1. `Promote(bool, bool) = u8`.
2. If `domain(A) != domain(B)`, result dtype MUST be the dtype of the operand in the higher-precedence domain.
3. If `domain(A) == domain(B)` and domain is not `bool`, result dtype MUST be the higher-precedence dtype within that domain.

Consequences required by this rule include:

1. `Promote(i8, u8) = i8`.
2. `Promote(u8, i8) = i8`.
3. `Promote(f16, bf16) = bf16`.
4. `Promote(complex64, f64) = complex64`.

Operand conversion and failure behavior:

1. A binary operation on operand dtypes `A` and `B` MUST select
   `P = Promote(A,B)` once for the operation.
2. For each logical result element, the left operand and then the right operand
   MUST be converted to `P` before arithmetic evaluation.
3. `bool` conversion to numeric MUST map `false -> 0`, `true -> 1`.
4. Implicit integer-to-integer conversion that cannot represent the source value
   in `P` MUST fail with `INT_OVERFLOW` before that element's arithmetic is
   evaluated.
5. Integer-to-float, float-to-float, and real-to-complex conversions follow
   IEEE-754 value semantics (precision loss allowed). Real-to-complex
   conversion sets the destination imaginary component to positive zero.
6. Arithmetic MUST then execute with destination dtype `P`, and every successful
   result element MUST be stored as `P`. Fixed-width integer arithmetic therefore
   checks representability in `P` at the operation itself; it MUST NOT use a wider
   accumulator whose only representability check occurs at final storage.
7. An implementation MAY use wider internal hardware operations only when the
   observable value and failure behavior is exactly equivalent to Rules 1-6.
8. Binary mixed-domain promotion MUST be order-independent
   (`Promote(A,B) == Promote(B,A)`).

`Nat` and `PositiveNat` are domain scalar types, not numeric dtypes. They MUST NOT participate in `Promote(A,B)`, tensor element dtype promotion, or numeric primitive execution.


### Floating Comparison Semantics (Unified Total Order)

For floating values (`f16`, `bf16`, `f32`, `f64`) in comparison contexts, implementations MUST use one shared total-order rule across:

1. expression operators (`==`, `!=`, `<`, `<=`, `>`, `>=`),
2. trait-level comparison semantics (`Eq`, `Ord`),
3. sort-key comparison contexts (`sort`, `sort_by`, `sort!`, `sort_by!`),
4. float-key hashing semantics used by `Hash` implementations.

Rules:

1. Any NaN compares greater than every non-NaN floating value.
2. Floating equality/order MUST be defined by one deterministic `total_key` per value.
   1. `total_key` MUST distinguish signed zero (`-0.0` vs `+0.0`).
   2. `total_key` MUST distinguish NaN payload/sign bit patterns.
3. Signed zero is ordered and compared distinctly: `-0.0 < +0.0` and `-0.0 != +0.0`.
4. For non-NaN values other than signed-zero tie points, ordering follows numeric order (including infinities).
5. Comparison operators and trait methods MUST be coherent with this order:
   1. `x == y` iff `Ord.cmp(&x, &y) == Ordering::Equal`,
   2. `x < y` iff `Ord.cmp(&x, &y) == Ordering::Less`,
   3. `x > y` iff `Ord.cmp(&x, &y) == Ordering::Greater`.
6. Float hashing MUST be coherent with this equality:
   1. if `x == y`, then `Hash.hash(&x, seed) == Hash.hash(&y, seed)` for every `seed`,
   2. conforming implementations SHOULD derive float hash from the same `total_key`.
7. Floating literal `0.0` denotes positive zero (`+0.0`).

This comparison policy is intentionally distinct from IEEE predicate semantics for NaN and signed-zero equality in v0.1. Numeric arithmetic and classification behavior remain IEEE-754 as specified elsewhere in this file.

### Complex Equality Semantics

`complex64` and `complex128` implement `Eq`. Two complex values are equal iff
both their real components and their imaginary components are equal under the
unified floating equality rules above, using `f32` components for `complex64`
and `f64` components for `complex128`. Complex equality therefore preserves
the same signed-zero and NaN bit-pattern distinctions componentwise.

`complex64` and `complex128` also implement `Hash`. Complex hashing starts with
the caller-supplied seed, hashes the real component's `total_key`, then hashes
the imaginary component's `total_key`. This order and component identity are
normative; the exact numeric result remains context-dependent as specified by
`std::prelude::Hash`.


### Fractional Division Operator (`/.`)

`/.` is a dedicated fractional division operator for scalar and tensor expression arithmetic.

`/.` MUST evaluate in floating/complex domain and MUST NOT use integer-division trap semantics.

Define helper `FloatLift(T)`:

1. `FloatLift(bool) = f16`.
2. `FloatLift(u8) = f16`, `FloatLift(i8) = f16`.
3. `FloatLift(u16) = f16`, `FloatLift(i16) = f16`.
4. `FloatLift(u32) = f32`, `FloatLift(i32) = f32`.
5. `FloatLift(u64) = f64`, `FloatLift(i64) = f64`.
6. `FloatLift(f16) = f16`, `FloatLift(bf16) = bf16`, `FloatLift(f32) = f32`, `FloatLift(f64) = f64`.
7. `FloatLift(complex64) = complex64`, `FloatLift(complex128) = complex128`.

Define result carrier `FDivCarrier(A,B)`:

1. Let `A' = FloatLift(A)`, `B' = FloatLift(B)`.
2. If both `A'` and `B'` are real floating dtypes, result dtype MUST be the higher-precedence float in `f16 < bf16 < f32 < f64`.
3. If either side is complex, result dtype MUST be:
   1. the complex side dtype when exactly one side is complex;
   2. the higher-precedence complex dtype when both sides are complex (`complex64 < complex128`).

Normative behavior:

1. `x /. y` result dtype MUST be `FDivCarrier(dtype(x), dtype(y))`.
2. Both operands MUST be converted to that carrier before division.
3. Divide-by-zero and overflow behavior MUST follow IEEE-754 floating/complex semantics (Inf/NaN), not `INT_DIV_BY_ZERO`.
4. Non-numeric operands (including `Tri`) are compile-time type errors (`NUMERIC_OPERAND_NOT_NUMERIC`).


### Numeric Runtime Edge Semantics

These rules are normative for fixed-width scalar expression execution, Core Tensor IR primitives, and all high-level tensor operations lowered from them.

1. Checked integer arithmetic is the default behavior:
   1. For fixed-width integer dtypes (`i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`), operations whose mathematical result is not representable in the destination dtype MUST fail with `INT_OVERFLOW`.
   2. Potentially faulting scalar operator syntax MUST use the lexical `checked {}` boundary from `../language/failure.md`; tensor primitives use their declared `Res[..., NumericError]` channels.
2. Integer division and remainder use one scalar/tensor-shared law after numeric
   promotion:
   1. For dividend `x` and nonzero divisor `y`, let `q` be the mathematical
      quotient truncated toward zero and let `r = x - q * y`.
   2. `/` and tensor `div` MUST produce `q`; `%` and tensor `rem` MUST produce
      `r`.
   3. The mathematical magnitudes satisfy `|r| < |y|`, and a nonzero `r` has
      the same sign as `x`.
   4. Integer `div` by zero MUST fail with `INT_DIV_BY_ZERO`.
   5. Integer `rem` by zero MUST fail with `INT_REM_BY_ZERO`.
   6. For the minimum value `MIN` of a signed dtype, `MIN / -1` and tensor
      `div(MIN, -1)` MUST fail with `INT_OVERFLOW`.
   7. `MIN % -1` and tensor `rem(MIN, -1)` MUST succeed with `0`; a backend
      divide trap MUST NOT change that language result.
3. Floating-point arithmetic follows IEEE-754 value semantics (no integer-style overflow traps):
   1. Floating `div` by zero MUST produce IEEE-754 infinities or NaN as applicable.
   2. Floating overflow MUST produce IEEE-754 infinities.
   3. Domain-invalid operations (for example `sqrt(-1)`, `log(-1)`, `0.0/0.0`) MUST produce NaN.
   4. Signed-zero behavior MUST follow IEEE-754 (for example `1.0 / -0.0` yields negative infinity and `1.0 / +0.0` yields positive infinity).
4. Numeric cast behavior is checked and deterministic:
   1. Source/destination legality state and value-domain validity MUST follow `cast-matrix.md`.
   2. `FORBIDDEN` dtype pairs MUST fail with `CAST_INVALID_DTYPE_PAIR`.
   3. Legal dtype pairs with invalid source values MUST fail with `CAST_INVALID_VALUE`.
   4. `astype` MUST remain checked value-conversion semantics and MUST NOT silently apply modulo-wrap or bit-pattern reinterpret behavior.
   5. Floating-to-integer conversion MUST use truncation toward zero when the input is finite and representable in destination range.
   6. Floating-to-integer conversion MUST fail with `CAST_INVALID_VALUE` when input is NaN, infinity, or outside destination range.
   7. Integer narrowing conversion MUST fail with `CAST_INVALID_VALUE` when source value is outside destination range.
   8. Integer widening conversion MUST succeed when destination range contains source range.
9. Integer-to-float and float-to-float conversions MUST follow IEEE-754 value semantics.
   10. Unary real-domain primitives (for example `sqrt`, `log`) MUST NOT implicitly promote real inputs to complex outputs in v0.1.
   11. Binary arithmetic and contraction paths MUST follow the numeric promotion lattice above; mixed real/complex binary operations MAY therefore yield complex outputs.
   12. `wrapping_cast` is an explicit modulo cast and MUST satisfy all of:
      1. source and destination dtypes are fixed-width integer dtypes;
      2. conversion uses modulo `2^bits(dst)` two's-complement semantics;
      3. conversion MUST NOT emit `CAST_INVALID_VALUE`;
      4. invalid dtype pairs MUST fail with `CAST_INVALID_DTYPE_PAIR`.
   13. `bitcast` is an explicit bit-pattern reinterpret cast and MUST satisfy all of:
      1. source and destination dtypes are fixed-width numeric dtypes in integer, floating, or complex domains;
      2. source and destination bit width are equal;
      3. conversion preserves per-element bit pattern exactly (no numeric conversion);
      4. invalid dtype pairs or unequal bit width MUST fail with `CAST_INVALID_DTYPE_PAIR`.
5. Error behavior in this section MUST be profile-invariant:
   1. `default` and supported `strict` execution MUST agree on error-vs-non-error outcomes and error class for identical logical inputs, apart from the explicit `DETERMINISM_UNAVAILABLE` availability failure.
6. Intentional modular arithmetic MUST be explicit:
   1. Implementations MUST provide the scalar `std::prelude::{wrapping_add, wrapping_sub, wrapping_mul}` family and tensor `std::tensor::prim::{wrapping_add, wrapping_sub, wrapping_mul}` family.
   2. Wrapping APIs MUST use two's-complement modulo `2^bits` semantics and MUST NOT emit `INT_OVERFLOW`.
7. The only v0.1 scalar numeric policy block is `checked {}`; generalized policy context syntax (for example `with numeric(...)`) is not part of v0.1.


### Empty Tensor Reduction Domains

For a tensor reduction, let the normalized selected-axis set be the axes left
after ordinary selector resolution and operation-specific normalization. The
reduction volume is the product of their input extents; the product of an empty
selected-axis set is `1`. An empty reduction domain is therefore a selected-axis
set whose reduction volume is `0`, not merely an input or output tensor with zero
elements.

The v0.1 empty-domain contract is:

1. Ordinary axis and shape validation runs before the operation-specific result
   below. A successful empty-domain result remains subject to the operation's
   dtype contract.
2. For each existing output coordinate, primitive `sum`, default `reduce{...}`,
   and the sum stage of `contract{...}` or `einop{...}` MUST produce the additive
   identity in the already-selected result dtype:
   1. fixed-width integer identity is `0`,
   2. real floating identity is positive zero (`+0.0`),
   3. complex identity has positive-zero real and imaginary components.
3. Primitive `mean` over an empty reduction domain MUST produce NaN at each
   existing output coordinate while preserving the supported input dtype:
   1. a real floating result is NaN in that floating dtype,
   2. a complex result has NaN real and imaginary components in its component
      dtype,
   3. NaN payload and sign are not fixed by this semantic rule.
4. Primitive reduction-form `max` has no v0.1 empty-domain identity. If its
   reduction volume is `0`, the complete operation MUST fail with
   `REDUCTION_EMPTY_DOMAIN`, using `ShapeError` at runtime. This validation runs
   even when the retained-axis output has no coordinates.
5. When the retained-axis output has no coordinates, operations with a defined
   empty-domain result (`sum`, default `reduce{...}`, `mean`, `contract{...}`,
   and `einop{...}`) MUST return the correctly shaped empty owning tensor; no
   identity or NaN element is materialized because no output coordinate exists.
6. An empty selected-axis set has reduction volume `1` and MUST be an owning
   no-op for primitive `sum`, reduction-form `max`, and `mean`: shape, logical
   values, dtype, axis order, and overlay are preserved. `keepdims` has no effect
   because no axis is selected.
7. With a non-empty selected-axis set, `keepdims=false` removes selected axes and
   `keepdims=true` retains them as size-1 axes under the ordinary primitive
   reduction overlay rules. The empty-domain result does not alter that shape or
   overlay behavior.
8. These results are profile-invariant. An implementation MUST NOT substitute a
   backend-specific infinity sentinel for empty `max`, widen an empty sum's
   result dtype, or emit a warning in place of the specified value or failure.
9. User-supplied `reduce_by` callbacks remain outside this fixed identity table.
   Empty phase subtensors are passed to the callback, whose `Res` result defines
   success or failure.


### Checked Integer Reduction Order

This baseline applies to non-empty fixed-width integer reduction slices evaluated
by primitive `sum`, the default `reduce{...}` reducer, and the sum stage of
`contract{...}` and `einop{...}`. It does not change floating-point reduction
semantics or user-supplied `reduce_by` callbacks.

For each applicable operation:

1. Reduction result dtype `R` is the dtype already selected by that operation:
   1. primitive `sum` and default `reduce{...}` use the input element dtype,
   2. `contract{...}` and `einop{...}` use the result dtype selected by their
      existing multiply-and-sum promotion rules.
2. The canonical reduced-axis order MUST be:
   1. increasing source `AxisIndex` order for primitive `sum`, after every
      `AxisName` has been resolved; the spelling order of a `Vec[AxisSel]` MUST
      NOT affect execution,
   2. left-hand-side axis order for default `reduce{...}`,
   3. `red_labels` order from the normative contract lowering for
      `contract{...}`,
   4. `red_keys` order from the authoritative `einop` normalization and
      lowering algorithm for each output.
3. Outputs MUST be evaluated in logical row-major order over their output axes,
   with the last output axis varying fastest. Multi-output `einop{...}` MUST
   evaluate outputs in right-hand-side source order.
4. Within one output coordinate, contributor coordinate tuples MUST be visited
   in logical row-major order over the canonical reduced-axis order, with the
   last reduced axis varying fastest.
5. The first successfully evaluated contributor initializes an accumulator of
   dtype `R`; the implementation MUST NOT insert an implicit zero addition for
   a non-empty slice.
6. Every later contributor MUST be added to that accumulator immediately using
   checked addition in `R`. The first non-representable intermediate result MUST
   fail with `INT_OVERFLOW`.
7. For `contract{...}` and `einop{...}`, each contributor is first produced by
   the operation's normative multiply lowering, including final-dtype
   selection before direct operand conversion and checked multiplication. Only
   a successfully produced contributor enters the checked sum fold in `R`.
8. Implementations MUST NOT reassociate the fold, accumulate in a wider dtype,
   or defer representability checking until final storage. A backend MAY use a
   different physical schedule only when the observable value, success versus
   failure outcome, primary diagnostic data, and first failing logical step are
   exactly equivalent to Rules 1-7.
9. `default` and `strict` determinism profiles use this same checked integer
   baseline.

Empty reduction slices do not have a first contributor and are therefore outside
this non-empty fold rule. Their result is governed by the empty-domain contract
above.


### Error Propagation and Precedence

These rules standardize observable error selection across primitive and lowered composite tensor execution.

1. Validation and execution stages MUST be evaluated in this precedence chain:
   1. schema/axis/shape/index/dim-solver validation,
   2. dtype/device compatibility validation,
   3. explicit per-call determinism profile availability validation (including strict unavailability),
   4. runtime primitive execution.
2. If multiple failures are possible, the earliest stage in this chain MUST determine the emitted primary error.
3. For an explicit profile-bearing request, `DETERMINISM_UNAVAILABLE` MUST be
   raised before element-value execution begins. Profile-less active strict
   context establishment remains the pre-execution activation/certification
   boundary defined in the determinism section, not a source value-channel
   failure.
4. Runtime primitive faults MUST be fail-fast over deterministic logical traversal order; the primary failure index is the first failing logical index.
5. If one logical element simultaneously satisfies multiple numeric fault predicates, primary code precedence MUST be:
   1. `INT_DIV_BY_ZERO`,
   2. `INT_REM_BY_ZERO`,
   3. `INT_OVERFLOW`,
   4. `CAST_INVALID_DTYPE_PAIR`,
   5. `CAST_INVALID_VALUE`.
6. For lowered composite operations (`reduce`, `contract`, `einop`, indexing composites):
   1. pre-execution failures MUST use operation-level validation codes when such codes are defined,
   2. execution-stage failures MUST preserve primitive runtime error code identity.
7. Value observability is expression-atomic:
   1. a failed expression yields no value,
   2. partially computed intermediate tensor buffers MUST NOT be observably exposed as successful results.
8. Expression-atomic value semantics do not imply rollback of explicit bang side effects outside pure tensor value evaluation.
