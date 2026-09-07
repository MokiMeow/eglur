# Effect Semantics

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Effect Categories

An effect is any externally observable interaction or non-local state change.

The following effect categories are normative and non-exhaustive:

1. `IO`: file, network, process, console interaction.
2. `STATE`: mutable memory, global state, externally visible cache mutation.
3. `NONDET`: time, randomness, environment dependence, scheduler dependence.

The v0.1 summary lattice is closed over these three categories. Non-exhaustive
means that a later language revision may add categories; a v0.1 implementation
MUST NOT add implementation-defined summary categories.

### Compile-time Effect Summaries

Every resolved callable has one compile-time effect summary:

```text
EffectSummary =
  Unknown
  | Known {
      categories: Set[IO | STATE | NONDET]
      state_scope: none | localizable | nonlocal
    }
```

The source language has no syntax for these summaries in v0.1. They are
compiler facts used to check calls and module boundaries; `!` remains the only
source-level effect authorization marker.

Rules:

1. `Known` with an empty category set and `state_scope = none` is the pure
   summary.
2. `state_scope = none` MUST be used exactly when `STATE` is absent.
3. `state_scope = localizable` means every summarized state mutation is
   reachable only through a mutable receiver or mutable argument supplied by
   the caller.
4. `state_scope = nonlocal` means at least one summarized state mutation can
   reach global storage, externally visible cache state, retained aliases, or
   any other state not confined to mutable receiver/argument roots.
5. Mutation rooted in fresh, non-escaping local storage contributes no
   `STATE` category to the enclosing callable's summary.
6. `Unknown` means that no sound `Known` summary is available. It is the
   conservative top value and MUST NOT be treated as pure or localizable.
7. Known summaries form a finite join lattice:
   1. category sets join by union,
   2. state scopes join by `none < localizable < nonlocal`,
   3. joining any summary with `Unknown` produces `Unknown`.
8. Implementations MAY encode this lattice differently, but MUST preserve its
   accept/reject behavior.


### Suffix Contract

Any function or method that can perform a category effect MUST have a `!` suffix.

Any function or method without `!` MUST be observationally pure.

Extern symbol declarations are a v0.1 exception to suffix surface:

1. names declared through `extern "<abi>" fn ...` are ABI-facing and MAY be non-bang identifiers,
2. this does not make extern calls pure; extern calls remain effectful by declaration kind.

For any non-bang function `f` and input `x`, the observable event set MUST be empty:

\[
Obs(f(x)) = \varnothing
\]

### Runtime-error Channel Boundary

Unrecoverable runtime termination is an externally observable effect.

Rules:

1. Non-bang functions/methods MUST NOT have execution paths that terminate via `runtime_error`.
2. In v0.1 baseline, explicit runtime-fatal operations (for example `std::test::abort!`) are bang-only surfaces and therefore forbidden in non-bang contexts by normal effect checking.
3. Recoverable failures in non-bang contexts MUST be represented through value channels (`Res`/`Opt`), not `runtime_error`.
4. `checked {}` scalar numeric faults satisfy this rule by returning `Err(NumericError)`; entering a `checked` block is not an observable effect and does not require a bang function.

### `Never` and Effect Checking

`Never` describes normal continuation, not effect authorization.

Rules:

1. A callable returning `Never` may be pure or effectful. Its bangness and
   summary are determined by the same operations and suffix rules as any other
   callable; for example, pure unbounded recursion may return `Never`, while
   `std::test::abort!` is bang because fatal runtime termination is observable.
2. Calling a `Never`-returning callable contributes that callable's ordinary
   effect summary before the call's no-normal-continuation fact is propagated.
3. Syntactically present code after a no-normal-continuation expression and in
   unselected or impossible branches remains subject to effect checking and
   conservative summary construction. v0.1 does not use dead-code elimination
   or path-value prediction to hide a bang call or remove a may-effect fact.
4. `Never` adds no source effect annotation, effect category, capability token,
   or exception to the context rule. `!` remains the only v0.1 source-level
   effect marker.

### Local-only Mutation in Non-bang Context

v0.1 treats purely local, non-escaping mutation as non-observable implementation state.

Rules:

1. Mutation of storage rooted in a function-local value MAY occur in non-bang context when that root is proven non-escaping in the current function body.
2. A local root is considered escaping if it is:
   1. returned or yielded,
   2. captured by a closure value that escapes,
   3. stored into non-local storage,
   4. passed through a mutable alias path that is not proven local-rooted.
3. Once a root is classified as escaping, further mutation through that root in the same non-bang body MUST fail with `EFFECT_VIOLATION`.
4. Non-local state mutation remains effectful and therefore forbidden in non-bang context.
5. A closure capture environment is not storage local to one closure invocation.
   Mutation of a captured binding or storage rooted in one is rejected with
   `MUTABLE_CAPTURE_FORBIDDEN` under `calls-references.md`, before this
   local-state-only exception is considered.

This exception does not permit `IO` or `NONDET` effects.


### Hidden Effects

Non-`!` APIs MUST NOT expose hidden observable cache behavior.

Internal optimizations MAY exist only when they are semantically unobservable.

For pure tensor execution, cross-identity variation permitted by numeric
tolerance is not the observational equality of one call. The
fixed-execution-identity contract in `../stdlib/conventions.md` requires
bitwise-equivalent value, post-state, and error observations for repeated
evaluations with identical inputs.
Therefore:

1. a scheduler, autotuner, dispatch cache, or kernel race that changes result
   bits after the execution plan identity is fixed contributes `NONDET`;
2. a non-bang tensor operation MUST NOT contain such a path;
3. compiler optimizations such as common-subexpression elimination MAY rely on
   exact repeatability only within that fixed identity; and
4. a different valid result produced under a distinct fixed execution identity
   is judged by the cross-identity profile criteria, not treated as a hidden
   runtime effect. One active artifact instance cannot switch identities
   between calls to obtain this exception.


### Standard Library Naming Requirements

The standard library MUST expose mutating variants with `!` suffix names.

The standard library MUST NOT expose mutating operations through non-bang names.

When both mutating and non-mutating forms exist, they SHOULD be provided as explicit pairs (for example, `sort` and `sort!`).


### Higher-order API Purity Split

To avoid hidden effect channels in pipelines, core and standard-library higher-order APIs are either pure-only adapters or explicit pure/effectful split families:

1. Non-bang pure-only adapters MUST accept only non-bang callbacks and have no required bang sibling.
2. Non-bang members of split families MUST accept only non-bang callbacks.
3. Bang combinators MAY accept effectful callbacks.
4. Surface `Fn(...) -> ...` parameter types are pure-only in v0.1.
   Bang APIs that accept effectful callbacks MUST specify that callback contract directly in their API rules rather than through a surface `Fn!` type.

At minimum, the standard library MUST provide:

1. `tap!(f)`: bang; callback MAY perform effects and the original value/sequence is preserved.

Optional pure inspection combinators MAY be provided. If provided, they MUST preserve original values and encounter order and MUST reject effectful callbacks (`PURE_COMBINATOR_EFFECTFUL_CALLBACK`).

Sequence fold/scan families follow the same split:

1. Pure: `fold`, `fold_res`, `scan`, `scan0` MUST reject effectful callbacks (`PURE_COMBINATOR_EFFECTFUL_CALLBACK`).
2. Bang: `fold!`, `fold_res!`, `scan!`, `scan0!` MAY accept effectful callbacks.

### Ownership and Effect Authorization

v0.1 keeps ownership and effect authorization orthogonal.

1. `Own[T]` MUST be treated as ownership/discharge metadata, not an effect capability.
2. Bangness remains the only core-language effect authorization marker in v0.1.
3. Explicit capability tokens and capability-typed parameters are not part of v0.1.


### Effect Violations

Compilers MUST emit diagnostics for non-`!` to `!` violations when statically provable.

Dynamic-link effect-enforcement boundary is fixed in v0.1:

1. Dynamically linked call targets (including extern-backed symbols) use declaration metadata classification exactly as ordinary call targets.
2. Non-bang callers MUST be rejected at compile-time with `EFFECT_VIOLATION` when calling effectful targets under the normal checking rules.
3. v0.1 baseline does not require runtime side-effect observation/probe instrumentation to re-classify dynamically linked calls.
4. Runtime channel handling for linkage resolution failures follows `unsafe-ffi.md` and `../errors/code-channels.md` (`EXTERN_LINKAGE_FAILED` channel split), not a separate dynamic-link effect code path.


### Static Effect-Checking Algorithm

Effect checking is a compile-time pass over typed AST after name resolution.

Implementations MAY use an equivalent internal representation, but observed accept/reject behavior MUST match this algorithm.

#### Precondition: Desugaring

Before effect checking, call-surface desugarings defined in `calls-references.md`
MUST be applied (or checked as-if applied), including `then` / `apply`.

Property-read form `recv.name` -> `recv.get_name()` for declared property members
MUST also be applied (or checked as-if applied), per `bindings-structs.md`.

`then` and `apply` MUST NOT introduce any special effect exemption.

#### Bangness Classification

1. Declared function/method names ending with `!` are effectful callables.
2. Declared function/method names without `!` are pure callables.
3. Extern declarations (`extern "<abi>" fn ...`) are effectful callables regardless of identifier suffix.
4. Trait method bangness is taken from trait declaration and MUST match impl declaration (`TRAIT_EFFECT_MISMATCH`).
5. Imported callables use their declared bangness/effect metadata from resolved symbol metadata.
6. Method receiver-signature legality is validated under `calls-references.md` reference-notation rules:
   1. malformed receiver shape MUST fail with `METHOD_RECEIVER_INVALID`,
   2. invalid `Self` scope MUST fail with `SELF_TYPE_INVALID`,
   3. receiver bangness/mutability mismatches MUST fail with `EFFECT_SIGNATURE`,
   4. non-receiver mutable-parameter violations continue to use `NON_BANG_MUT_PARAM_FORBIDDEN`.

Bangness and effect summary are separate facts. A bang callable MAY have a
known pure summary, but remains a bang call surface. A non-bang callable MUST
have the known pure summary after local-only mutation has been discharged;
otherwise its declaration fails with `EFFECT_VIOLATION`.

#### Summary Construction and Propagation

Callable summaries are computed over the resolved call graph.

1. Direct `IO` and `NONDET` operations contribute their respective categories.
2. A direct state mutation contributes:
   1. no category when rooted in proven non-escaping local storage,
   2. `STATE` with `localizable` scope when rooted only in the callable's
      mutable receiver or mutable parameters,
   3. `STATE` with `nonlocal` scope otherwise.
3. At each call site, the callee summary is transferred into the caller:
   1. `IO` and `NONDET` categories propagate unchanged,
   2. `STATE/nonlocal` propagates as `STATE/nonlocal`,
   3. `STATE/localizable` contributes no category when every corresponding
      mutable receiver/argument path is rooted in proven non-escaping local
      storage,
   4. otherwise, `STATE/localizable` remains localizable only when every
      remaining mutable path is rooted in the caller's mutable receiver or
      mutable parameters; it becomes nonlocal for any other path,
   5. `Unknown` propagates as `Unknown`.
4. Summary transfer uses the resolved callable identity. Method syntax,
   callable references, hole-based partial application, import aliases, and
   public re-exports MUST NOT create a new summary identity.
5. Direct and mutually recursive callables are summarized by deterministic
   least-fixed-point iteration over each call-graph strongly connected
   component:
   1. start each source-defined callable in the component at the known pure
      summary,
   2. repeatedly apply the transfer and join rules above,
   3. stop when the finite lattice stabilizes.
6. Compiler-known primitives and standard-library declarations MUST provide
   trusted summaries under the same lattice.
7. A callable whose required declaration metadata is absent, malformed, or
   incompatible with this contract has summary `Unknown`.
8. Extern declarations have summary `Unknown` in v0.1. An `unsafe` block
   authorizes the unsafe operation but does not refine that summary.
9. Trait-method calls are summarized as follows:
   1. a call whose receiver type selects one concrete impl uses that impl
      method's inferred summary, or the selected default body's inferred
      summary when the impl does not override it,
   2. a non-bang method requirement on an unresolved bounded generic receiver
      has the known pure summary because every conforming impl is required to
      preserve non-bang purity,
   3. a bang method requirement on an unresolved bounded generic receiver has
      summary `Unknown`,
   4. a generic callable containing such an unresolved bang call therefore
      has summary `Unknown`; v0.1 does not refine that callable's summary when
      a later instantiation selects a concrete impl,
   5. `closed trait` authority does not change these rules or permit a
      whole-program join over currently known impls.
10. Calls through catalog callback contracts are summarized at the call site:
    1. begin with the catalog callable's trusted own summary,
    2. `callback[pure]` contributes the known pure summary,
    3. each `callback[effectful]`, `callback[scoped-resource]`, or
       `callback[task-thunk]` argument that the API may invoke contributes the
       actual callback expression's summary,
    4. callback summaries are transferred and joined by the ordinary rules
       above, and any `Unknown` callback makes the complete call summary
       `Unknown`,
    5. an inline closure is summarized from its body; a named callable,
       callable reference, or partial application uses its resolved summary,
    6. a callback summary is included even when runtime data may cause zero
       invocations, because v0.1 summaries are conservative may-effect facts
       and do not use path-sensitive value prediction,
    7. this call-local substitution does not create a source `Fn!` type, a
       storable effect-polymorphic callable, or an open-ended generic effect
       formula.

#### Trusted Standard-Library Summaries

`../stdlib/effect-summaries.yaml` is the sole authored authority for v0.1
standard-library own summaries and callback propagation metadata.

Rules:

1. Its ordered `sources` list MUST exactly match the baseline module documents
   indexed by `../STDLIB.md`; unindexed module documents MUST NOT enter this
   catalog.
2. Every non-bang, callback-free callable receives the catalog's known-pure
   default own summary. This default is valid only because non-bang declarations
   are required to be observationally pure.
3. Every bang callable and every callback-taking callable MUST have one explicit
   source-keyed record. A missing, duplicate, or unmatched record is malformed
   trusted metadata and therefore fails specification-package validation.
4. An explicit record stores the callable's own summary separately from every
   named callback position. Callback kinds MUST exactly match the corresponding
   `eglur-catalog` declaration.
5. Every known profile MUST satisfy the closed lattice above. `Unknown` carries
   neither categories nor a state scope.
6. Callable re-exports MUST resolve to their target callable identity. Every
   member of a re-exported overload set MUST have identical own-summary and
   callback metadata; heterogeneous target metadata is invalid.
7. Optional declarations inside a baseline module document are covered when
   provided, but this metadata does not make them required. Declarations in
   documents outside the baseline module index remain outside the v0.1 trusted
   catalog.
8. The catalog adds no source annotation or authorization syntax. `!` remains
   the only v0.1 source-level effect marker.
9. Human-readable effect tables MUST be projected from the resolved machine
   catalog rather than maintained as a second authored mapping.

#### Context Rule

For each function/method body with context `ctx`:

1. If `ctx` is non-bang, every call target MUST have a known effect summary.
   An `Unknown` target MUST fail with `EFFECT_VIOLATION`, whether the target is
   source-defined, imported, indirect, or extern-backed.
2. If `ctx` is non-bang, calling a bang callable is forbidden by default and MUST fail with `EFFECT_VIOLATION`.
3. Non-bang MAY call a bang callable only when all of the following hold:
   1. the call has at least one mutable receiver/argument path,
   2. the call is classified as local-state-only (no `IO`, no `NONDET`, and no non-local `STATE`),
   3. every mutable receiver/argument path is rooted in non-escaping local storage in the current function body,
   4. effect summary classification is available at compile-time.
4. If local-state-only classification is unavailable or cannot be proven, non-bang call MUST fail with `EFFECT_VIOLATION`.
5. Non-bang function/method non-receiver parameter lists MUST NOT include `&mut` parameters (`NON_BANG_MUT_PARAM_FORBIDDEN`).
6. If `ctx` is bang, both bang and non-bang calls are permitted.

This rule applies uniformly to direct calls, method calls, and calls introduced by desugaring.

#### Higher-order Callback Rule

For APIs declared as pure combinators (non-bang, pure-callback contract):

1. Callback expressions MUST be effect-free under non-bang checking.
2. If callback body calls bang callables, compilation MUST fail with `PURE_COMBINATOR_EFFECTFUL_CALLBACK`.
3. This rule applies both to pure-only adapters and to non-bang members of split families.
4. Ordinary `Fn(...) -> ...` parameters are checked by callable compatibility from `calls-references.md`; effectful callback values supplied to those parameters fail with `EFFECT_VIOLATION`, not the pure-combinator-specific code.

For APIs declared as effectful combinators (bang variants):

1. Callback expressions MAY call bang callables.
2. The complete combinator call summary MUST include the actual callback
   summary under `Summary Construction and Propagation`.
3. A callback's effects MUST NOT be hidden by the combinator's own bang marker.

#### Partial Application and Captured Expressions

Hole-based partial application captures non-hole arguments at construction time.

1. Captured expressions are checked in the enclosing context where partial value is created.
2. Any forbidden bang call in captured expressions MUST fail using standard effect diagnostics:
   1. `EFFECT_VIOLATION` for ordinary non-bang context violations,
   2. `PURE_COMBINATOR_EFFECTFUL_CALLBACK` when violation is under pure-callback combinator contracts.
3. Invoking the resulting callable is checked by normal call rules at invocation site.
4. When the resulting callable is checked against a surface `Fn(...) -> ...` type, the target invocation itself MUST be pure.
   A bang-target partial application is not compatible with `Fn(...) -> ...` in v0.1.

#### Determinism of Diagnostics

When multiple effect violations exist in one function body, implementations MAY report one or more errors, but each emitted error MUST use stable codes/tokens from `../ERRORS.md`.
