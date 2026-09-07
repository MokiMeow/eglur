# Core Invariants

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Left-to-Right Composition and Normal Form

The canonical reading direction of expressions SHALL be left-to-right.

Specification examples SHOULD use chaining normal form as the primary form.

Equivalent formulations MAY be used for explanatory purposes only.


### Canonical Lexical Text Order

When a baseline contract requires lexicographic ordering of text, it MUST use
the following locale-independent order:

1. Compare the two sequences of Unicode scalar values from left to right by
   numeric scalar value.
2. The first unequal scalar value determines the order.
3. If one sequence is an exact prefix of the other, the shorter sequence sorts
   first. Two sequences compare equal only when they contain the same scalar
   values in the same order.
4. Implementations MUST NOT apply Unicode normalization, canonical-equivalence
   matching, case conversion, case folding, locale collation, or grapheme-cluster
   grouping before comparison.
5. The registry package-tree collision guard in `package.md` is a rejection-only
   portability check. It does not define lexical equality or ordering and does
   not alter this invariant.
6. Punctuation and path separators are ordinary scalar values. The comparison
   MUST NOT interpret path segments or filesystem hierarchy.
7. After text input has been decoded to its scalar-value sequence, comparing
   the unsigned bytes of that value's UTF-8 encoding lexicographically produces
   the same order and MAY be used as an equivalent implementation. Source
   spelling bytes, including escape spellings, MUST NOT be compared instead.

Normative boundary fixtures:

| Unordered inputs | Required ascending order | Boundary |
|---|---|---|
| `"aba"`, `"ab"` | `"ab"`, `"aba"` | exact prefix |
| `"a"`, `"A"` | `"A"`, `"a"` | ASCII case |
| `"\u{e9}"`, `"z"` | `"z"`, `"\u{e9}"` | ASCII and non-ASCII |
| `"\u{e9}"`, `"e\u{301}"` | `"e\u{301}"`, `"\u{e9}"` | no Unicode normalization |
| `"0"`, `"/"` | `"/"`, `"0"` | separator has no special treatment |


### Value vs Effect Separation

Operations that perform observable effects MUST be marked with the `!` suffix in their declared name.

Operations without the `!` suffix MUST NOT perform observable I/O, non-local mutable state updates, or nondeterministic actions.

Purely local non-escaping mutation MAY be used as implementation state under `effects.md` and does not change observable purity.

Every default-profile tensor result MUST satisfy its numeric baseline criteria.
Different valid floating/complex result bits MAY be exposed only across
distinct fixed execution identities. Within one fixed execution identity,
non-bang tensor operations remain exactly repeatable as required by
`effects.md` and `../stdlib/conventions.md`.

Pure `Fn(...) -> ...` closure values capture ordinary immutable values by
construction-time snapshot.
They MUST NOT capture mutable bindings or expose mutable capture state.

Implementations SHOULD statically reject violations where possible.


### Failure as Values

Recoverable failure MUST be represented with `Res[T, E]`.

Potential scalar fixed-width integer faults MUST become `Res[T, NumericError]` through an explicit lexical `checked {}` boundary; they MUST NOT be hidden traps.

Missing/optional values MUST be represented with `Opt[T]`.

Core program flow MUST NOT depend on uncaught exceptions.


### Bottom Type vs Structural Inhabitance

`Never` is the sole canonical bottom type and carries the exact
no-normal-continuation fact defined in `type-system.md`. Structural
`Uninhabited` analysis is a separate conservative fact used only for the closed
match coverage and reachability rules. It MUST NOT become general subtyping,
trait proof, layout/niche inference, foreign ABI, or implicit divergence.


### Ownership vs Effect Authorization

`Own[T]` and effect authorization are distinct semantic axes.

1. `Own[T]` denotes ownership/discharge obligations for values.
2. `Own[T]` alone MUST NOT grant permission to perform `IO`/`NONDET` effects.
3. In v0.1, effect authorization remains governed by bang rules and explicit unsafe/FFI boundaries.
4. Future capability-token extensions MAY add explicit effect-authorization requirements, but MUST NOT redefine Rule 1 ownership obligations.
5. v0.1 confines ownership to direct roots plus the built-in one-root `Opt`/`Res` carriers; ordinary aggregates, generic parameters, and associated type slots/projections remain ownership-free. Generic parameters and associated type slots are also reference-free, so borrowing remains visible outside the abstraction as `&T` or `&T::Item`.
6. `Box[T]` provides value-semantic layout indirection and MUST NOT inherit `Own[T]` discharge semantics.
7. Designated external resource payloads are acquired only as `Own[R]` roots and require an explicit bang terminal or scoped resource API for discharge.
8. For ownership-free, reference-free `T`, `Task[T]` is an ownership-free completion value, not a resource root; copying it preserves one computation identity and `join!` borrows it repeatably.
9. Local `&T` and `&mut T` values are non-escaping aliases rather than
   ownership roots. `&mut T` grants write access but not uniqueness; unique
   exact-once responsibility requires a by-value `Own[T]` transfer through
   `move`.


### General Language and Tensor Non-interference

Language-core semantics and required general standard-library and platform
contracts, including collections, I/O, concurrency, and ordinary pipelines,
MUST NOT depend on tensor axis, shape, schema, lowering, or backend facts.

Tensor support enters only through the closed first-class surface defined in
`implementation-boundary.md`. Hosting that surface in the language grammar MUST
NOT change ordinary identifier lookup, call inference, trait solving, or
non-tensor runtime requirements.

Tensor-specific axis and metadata invariants are owned by `tensor-model.md` and
`../axes.md`, not by the general core invariant set.


### Traits over Inheritance

Implementation inheritance is forbidden (`INHERITANCE_FORBIDDEN`).

Traits MUST NOT declare `field` or `prop` members (`TRAIT_STATE_FORBIDDEN`).

Conflict resolution for trait default methods MUST be explicit and compile-time checked.

Capability-polymorphic source APIs MUST expose a named generic parameter and its nominal trait bounds; associated result types MUST use explicit projections such as `T::Item`.

Closed traits restrict impl authority to the declaring locality boundary. In
package mode, package equality is `PackageInstanceId` equality under
`package.md`, not `PackageName` equality; one explicit file-set invocation is
one locality boundary under `modules-resolution.md`. This restriction MUST NOT
introduce specialization, negative reasoning, or hidden carrier unions.

Ordinary trait-impl authority is determined by the canonical trait declaration
or the alias-expanded nominal target head. Generic arguments, nested local
types, structural wrappers, and source aliases MUST NOT create another locality
boundary or impl authority.
