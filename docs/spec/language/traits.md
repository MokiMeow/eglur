# Traits, Coherence, and Dispatch

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Trait Model

Traits MAY include default methods.

Traits MAY declare non-generic associated types with `type Name`.

Traits MUST NOT include instance state.
In v0.1 this forbids both `field...` and `prop...` member declarations (`TRAIT_STATE_FORBIDDEN`).


### Explicit Generic Capability Model

The v0.1 source model for code that is generic over a capability is an explicit
named type parameter with nominal trait bounds:

```eglur
fn same[T: Eq](left: &T, right: &T) -> Bool = left == right
fn copied[T: Clone](value: &T) -> T = value.clone()
```

The bounded tensor source layer uses this same general mechanism rather than
altering trait solving. `ReadableTensor` is its standard closed readable-carrier
capability and is visible through the prelude:

```eglur
fn inspect[C: ReadableTensor](x: C) -> Nat = x.rank()
fn keep_elem[C: ReadableTensor](_x: C, value: C::Elem) -> C::Elem = value
fn identity[C: ReadableTensor](x: C) -> C = x

import std::tensor::AffineTensor
fn affine_identity[C: ReadableTensor + AffineTensor](x: C) -> C = x
```

Rules:

1. The generic carrier remains explicit in source and MAY appear unchanged in parameter and result positions.
2. A capability-dependent type is named through an associated-type projection such as `C::Elem` or `Self::Elem`.
3. Requirement-driven anonymous unification, implicit closed-carrier generics, and union-carrier signatures are not source typing models in v0.1.
4. A future `where` surface, qualified projections, richer associated-type constraints, or dynamic dispatch MAY extend this model without changing the meaning of the explicit v0.1 declarations.
5. `std::tensor::ReadableTensor` and `std::tensor::AffineTensor` are declared
   normatively by the standard-library catalog. The prelude re-exports only
   `ReadableTensor`; naming `AffineTensor` requires an explicit import.

The rejected alternatives are intentionally not aliases for this surface:

1. Catalog-only requirement unification cannot express a source function's same-carrier return type or a reusable source function value without hiding compiler behavior.
2. Anonymous forms such as `some ReadableTensor` hide the generic parameter that explicit call-site type arguments and same-carrier results need to name.
3. A closed `Union[Tensor[T], TensorView[T]]` loses the nominal capability extension point and does not preserve an arbitrary concrete carrier in `C -> C` signatures.

The canonical prelude `Ord` capability has one compiler-known prerequisite:
`std::prelude::Ord -> std::prelude::Eq`.

1. An `impl Ord for T` is valid only when the corresponding `T: Eq` obligation is satisfied.
2. A declared generic bound `T: Ord` makes the exact `T: Eq` obligation available in that generic declaration.
3. This fixed prelude prerequisite is not source-level supertrait syntax and does not permit user-defined trait implication edges.
4. No other v0.1 trait implication is inferred unless another normative prelude contract explicitly defines it.


### Associated Types and Projections

Associated types are deliberately first-order in v0.1.

Declaration and impl rules:

1. A trait associated type declaration is `type Name` and has no generic parameters, default, bound, or initializer.
2. Associated type names MUST be unique within one trait (`NAME_AMBIGUOUS` otherwise). Associated types occupy the trait's type-member namespace and MAY share a spelling with a callable member.
3. An `impl Trait for Type` MUST contain exactly one `type Name = TypeExpr` binding for every associated type declared by `Trait`.
4. Missing, unknown, or duplicate associated-type bindings MUST fail with `TRAIT_IMPL_ASSOC_TYPE_MISMATCH`.
5. An associated-type binding RHS is checked in the impl's generic scope and MAY refer to `Self` and to valid projections from bounded impl parameters.
6. The complete impl-header substitution is applied to an associated binding before projection normalization. A binding such as `type Item = T::Item` therefore remains symbolic while checking a generic impl and normalizes through `T`'s concrete impl when that generic impl is instantiated.
7. Before trait method signatures are compared, the impl's associated-type bindings MUST be substituted into both the trait requirement and impl method signature and then normalized under the rules below.
8. Associated-type binding completeness, projection validity, ownership-freedom, reference-freedom, visibility, and cycle validation are checked before method obligation and signature matching.
9. Associated type slots range only over ownership-free types in v0.1. A binding that normalizes to an ownership-root type or conditional ownership carrier MUST fail with `OWNERSHIP_TYPE_POSITION_FORBIDDEN`.
10. Associated type slots also range only over reference-free types as defined by `calls-references.md`. A binding whose alias-expanded, normalized structure contains `&` or `&mut` at any depth MUST fail with `REFERENCE_TYPE_POSITION_FORBIDDEN`; write the borrow outside the projection as `&T::Item` or `&mut T::Item` instead.
11. A symbolic binding such as `type Item = T::Item` inherits the declaring associated slot's reference-free guarantee. Every concrete binding is validated at its own impl declaration, so this rule does not require lifetime inference or post-substitution body checking.
12. Projection normalization performs cycle detection before either structural position check may depend on a cyclic result (`ASSOCIATED_TYPE_CYCLE` otherwise). For every finite normalized binding, ownership-position validation precedes reference-free validation when both constraints fail; both precede method obligation checking and normalized signature matching.
13. A conformance family is externally reachable when both its trait and target type head are public; language-intrinsic scalar and generic type heads count as public. After impl authorization and coherence, every impl generic bound, the complete implemented-trait reference, target type, and every finite normalized associated binding of an externally reachable conformance MUST satisfy the externally reachable type-contract closure in `modules-resolution.md` (`VISIBILITY_VIOLATION` otherwise). Transparent aliases are expanded without retaining source-alias visibility provenance: a private alias whose expansion is externally nameable is permitted, while a private declaration or foreign-package declaration lacking the impl module's required `pub import` is rejected. A conformance with a private trait or private target head may use module-private types in its header arguments and bindings.
14. An impl generic parameter in a binding, and a symbolic binding such as `T::Item`, inherit external nameability from the concrete target substitution. `T::Item` also inherits the associated-slot visibility invariant: each concrete externally reachable conformance selected for `T` is checked at its own impl declaration. A substitution private to another module does not make that concrete target externally nameable there; a caller-local private substitution remains nameable to its owner and does not invalidate the generic impl declaration. v0.1 does not create an opaque external projection and does not defer a declaration-site private-name failure to a use site.
15. After impl-header authorization and coherence checks succeed, the current `Self: Trait` header is an available assumption while checking that impl's associated bindings and methods. It does not prove a distinct recursive obligation cycle through other impls.

Validation within one trait impl uses this primary-failure order:

1. parse and resolve the implemented trait and target type, including contextual
   `Self`, projection-source legality, and generic error-union header legality,
2. validate impl authority and coherence in this order:
   1. closed-trait authority,
   2. the ordinary orphan rule,
   3. duplicate, overlap, and specialization classification,
   4. impl-parameter constraints,
3. validate externally reachable visibility of the impl generic bounds,
   implemented-trait reference, and target type,
4. validate associated-binding completeness before inspecting any supplied binding RHS,
5. for each required binding in source order, resolve names and aliases, validate projection syntax and arity, normalize with cycle detection, then validate ownership, references, and externally reachable visibility in that order,
6. validate required method presence, effect marker, and normalized signature.

An earlier class in this list is the primary diagnostic when the same impl violates multiple classes. Diagnostics within one class use source order and then `../errors/ordering.md`.

Projection rules:

1. v0.1 projection bases are limited to `Self` in a trait/impl owner context and a named generic parameter with declared bounds.
2. `Self::Name` resolves against the current trait while checking that trait or one of its impls. `Self::Name` outside an owner context fails first with `SELF_TYPE_INVALID` under `calls-references.md`.
3. `T::Name` resolves only when exactly one declared bound on `T` declares associated type `Name`. The declaring bound is the complete normalized trait reference, including its type arguments.
4. An unbound generic base, unknown associated name, or name declared by more than one bound MUST fail with `ASSOCIATED_TYPE_PROJECTION_INVALID`.
5. v0.1 has no qualified projection escape hatch. A declaration with colliding associated names MUST rename one associated type or split the generic declaration.
6. Source-written concrete-type projection such as `Concrete::Name`, associated-type equality constraints, associated-type defaults/bounds, and generic associated types are not part of v0.1. This source restriction does not prohibit the compiler-internal concrete projection produced when Rule 2 of normalization substitutes a generic base.
7. Source legality is checked after transparent alias expansion but before projection-value normalization. An alias-expanded projection retains projection-origin provenance, so an alias cannot make a projection legal in an impl header or erase its conservative overlap identity.
8. A projection path contains exactly one associated name after its base. Projection chaining such as `T::Item::Nested` MUST fail with `ASSOCIATED_TYPE_PROJECTION_INVALID`.
9. Associated types have arity zero in v0.1. After the base and associated name resolve successfully, `T::Item[U]` or in-scope `Self::Item[U]` MUST fail with `TYPE_ARG_ARITY_MISMATCH`; this syntax does not enable generic associated types.
10. Associated type declarations inherit the visibility of their trait and have no per-item visibility modifier in v0.1. Importing a public trait makes its associated names available through valid projections from that trait bound; associated names are not imported as standalone type identifiers.
11. Associated-type projections MUST NOT appear in an impl's implemented-trait reference or target type, including through transparent aliases. This keeps impl heads nominal and MUST fail with `ASSOCIATED_TYPE_PROJECTION_INVALID` when violated; the out-of-scope `Self` precedence from Rule 2 still applies to `Self::Name` written in an impl header.

Projection normalization and equality:

1. In a generic declaration body, a projection is represented canonically by its base parameter, complete declaring trait reference, and associated name. Repeated occurrences of that same canonical projection denote the same abstract type.
2. Normalization applies the current impl-header or call-site substitution to the canonical projection and then proceeds as follows:
   1. `Self::Name` in the current impl selects that impl's `Name` binding, even when the impl target remains generic,
   2. a bounded generic base with no concrete substitution remains a canonical symbolic projection,
   3. a substituted nominal base with symbolic arguments structurally matches the unique coherent impl head, infers that impl's parameters, and validates its bounds in the current generic well-formedness environment,
   4. a fully concrete substituted base selects the unique applicable impl through ordinary trait-obligation lookup,
   5. the selected impl's `Self` and generic parameters are substituted into its associated binding,
   6. transparent aliases and valid associated projections in the resulting type are recursively normalized.
3. Concrete normalized projections compare as their resulting ordinary types. Distinct unresolved canonical projections are not equal merely because known impls currently choose equal bindings.
4. Projection normalization MUST NOT run in reverse to infer a base type from a concrete argument, result, annotation, or expected-`Fn` component, nor may it select among overloads or impls by searching associated binding values.
5. For ordinary type equality, a concrete normalized projection compares as its resulting type. For impl-head legality and overload/coherence disjointness, projection-origin provenance survives alias expansion and normalization and MUST NOT be used as a negative fact.
6. Re-entering the same concrete or canonical symbolic projection obligation while Rule 2 is still resolving bindings in one impl declaration is a direct or indirect associated-type cycle and MUST fail with `ASSOCIATED_TYPE_CYCLE`.
7. v0.1 defines no separate later-concrete associated-cycle class. If selecting an external binding first encounters an unresolved recursive trait obligation, that obligation fails with `TRAIT_BOUND_NOT_SATISFIED`; it is not reclassified as an associated-type cycle.
8. Projection normalization failure caused by an invalid projection uses `ASSOCIATED_TYPE_PROJECTION_INVALID`; failure of a substituted nominal base, whether it still has symbolic arguments or is fully concrete, to find and satisfy its unique structural impl uses `TRAIT_BOUND_NOT_SATISFIED`.
9. After normalization, the resulting type is revalidated in its enclosing source position under `type-system.md` before layout analysis, signature comparison, or expression typing.


### Closed Traits

`closed trait` restricts impl authority without changing dispatch semantics.

For all rules in this section, package-mode declarations are in the same package
exactly when their owning modules have the same `PackageInstanceId` under
`package.md`. In file-set mode, all modules mapped by one explicit file-set
invocation form one locality boundary. Implementation-owned standard-library
declarations are outside either user locality boundary. `PackageName` equality
is never package locality.

1. Only source modules in the package instance that declares a closed trait MAY declare impls of that trait.
2. An impl in another package MUST fail with `CLOSED_TRAIT_IMPL_FORBIDDEN`, even when the target type is local to the implementing package.
3. The closed-trait package check occurs before the ordinary orphan-rule check.
4. Modules other than the declaring module but within the same package MAY implement the trait, subject to visibility and all ordinary coherence rules.
5. `closed` does not imply exhaustive pattern matching over implementors, specialization, negative reasoning, or dynamic dispatch.
6. `closed` is contextual only in the `closed trait` prefix; it remains an ordinary identifier elsewhere.


### Inherent Methods and Impl Form

Eglur uses the following declaration layout:

1. Inherent methods MUST be declared inside the owning `struct` body.
2. Top-level inherent impl blocks (`impl Type { ... }`) are not part of v0.1.
3. Trait implementations MUST be written as top-level `impl Trait for Type { ... }`.
4. Negative impl syntax (`impl !Trait for Type`) is unsupported in v0.1.


### Coherence Rules

#### Global Uniqueness

For any fully monomorphized `(Trait, Type)` pair, a program MUST observe at most
one applicable impl across the full package graph. The canonical identities of
nominal trait and type declarations include their owning `PackageInstanceId` in
package mode and their canonical file-set module identity in file-set mode.

#### Orphan Rule

Orphan locality is determined from canonical declaration ownership, not from a
type expression containing a local declaration somewhere inside it.

After name resolution, projection-source legality checking, and transparent
alias expansion, the compiler MUST classify the target as follows:

1. A nominal application `Head[...]` has the canonical nominal declaration
   `Head` as its **nominal target head**. A zero-argument nominal type is the
   same case with no type arguments.
2. Language-intrinsic and standard-library nominal constructors have their
   canonical implementation-owned head. They are not local to a user package
   or file-set merely because one of their arguments is local.
3. `Never`, a reference (`&T` or `&mut T`), tuple, function type, union, raw
   pointer, trait-object type, or bare generic parameter has no nominal target
   head.
   Forms forbidden elsewhere retain their earlier diagnostic under the
   validation order above.
4. Parenthesized type syntax does not change the classification.

The implemented trait is local exactly when its canonical trait declaration is
owned by the impl's locality boundary. The target type is local exactly when it
has a nominal target head whose canonical declaration is owned by that same
boundary. Therefore, for `impl Trait for Type`, at least one of the following
MUST be local:

1. the canonical trait declaration,
2. the nominal target head.

If both are non-local, compilation MUST fail with `ORPHAN_RULE_VIOLATION`.

`Never` receives no compiler-generated or prelude trait implementations merely
because it is uninhabited. An explicit `impl LocalTrait for Never` is permitted
only through the ordinary local-trait side of the orphan rule and remains
subject to coherence and complete method checking.

Consequences:

1. Type arguments and nested types never contribute orphan locality. A foreign
   `Vec[Local]` or `Vec[Opt[Local]]` head remains foreign.
2. A local generic head remains local for every valid argument vector. For
   example, `LocalBox[Foreign]` is local because `LocalBox` is local.
3. A transparent alias declaration, import alias, or re-export never creates
   impl authority. `type LocalVec[T] = Vec[T]` expands to the foreign `Vec`
   head, while an alias of `LocalBox[Foreign]` retains the local `LocalBox`
   head.
4. References and `Box[...]` do not receive a fundamental-wrapper exception in
   v0.1. In particular, `&Local` and `Box[Local]` are not local targets for a
   foreign trait.
5. `Tensor[...]` follows the same rule: its implementation-owned nominal head
   is not local to user code, regardless of its type arguments.
6. Orphan authorization occurs before an impl is inserted into or compared
   within duplicate, overlap, and specialization buckets. An unauthorized impl
   that would also overlap an owner-provided blanket impl reports
   `ORPHAN_RULE_VIOLATION`.

#### Error-Union Impl Header Rigidity

Trait impl headers do not infer an impl parameter from the members of a
structural error union.

Rules:

1. The compiler MUST expand transparent aliases and apply the error-union
   normalization rules in `type-system.md` before this check.
2. If an impl-introduced generic parameter occurs strictly below any remaining
   structural `Union[...]` node in the implemented trait reference or target
   type, compilation MUST fail with
   `IMPL_GENERIC_ERROR_UNION_UNSUPPORTED`.
3. An occurrence of the same parameter outside the union does not make the
   nested occurrence legal. In particular,
   `impl[E] Trait[E] for Res[Unit, Union[E, IoError]]` is rejected.
4. A parameter in the complete error position remains legal. For example,
   `impl[E] Trait for Res[Unit, E]` may bind `E` to an entire normalized error
   union under the ordinary whole-position matching rule.
5. A source union that normalizes to a non-union before Rule 2, such as
   `Union[E, Never]`, is governed by that normalized form.
6. A remaining structural union in a valid impl header is therefore rigid with
   respect to that impl's parameters. Coherence and obligation lookup compare
   its normalized member-set identity without member subtraction, partitioning,
   permutation search, or set unification.
7. Rule 2 is impl-header type legality. It is checked before closed-trait
   authority, orphan authorization, duplicate/overlap/specialization
   classification, and impl-parameter constraints. Transparent aliases cannot
   defer or change that precedence.

#### Overlap and Specialization

Two impl headers overlap if there exists a type substitution that makes both apply to the same `(Trait, Type)` pair.

Rules:

1. Exact duplicate headers, including alpha-normalized generic parameters and order-insensitive normalized bound sets, are forbidden (`TRAIT_IMPL_DUPLICATE`).
2. Overlapping headers are forbidden (`TRAIT_IMPL_OVERLAP`).
3. Strict specialization (a strict subset overlap) is also forbidden in v0.1 (`SPECIALIZATION_FORBIDDEN`).
4. Generic bounds are ignored when proving non-duplicate impl headers disjoint. Two headers whose trait/target type structure can unify overlap even when their declared bound lists differ.
5. Bounds MAY make one otherwise-valid impl inapplicable at a concrete obligation, but they MUST NOT be used as negative facts or specialization guards.
6. Associated-type bindings are impl-body obligations, not impl-header identity. Different binding RHS values MUST NOT make otherwise duplicate or overlapping impl headers disjoint.
7. A projection-origin type is forbidden in an impl head even when alias expansion and concrete projection normalization would yield an ordinary nominal or scalar type.

#### Blanket Impl Parameter Constraint

Each generic type parameter introduced by an `impl[...]` header MUST appear in at least one of:

1. The implemented trait path (including trait type arguments).
2. The target type expression after `for`.

Parameters that do not constrain either side are forbidden in v0.1 (`IMPL_UNCONSTRAINED_TYPE_PARAM`).

#### Tensor Coherence Guard

`Tensor[...]` implementations MUST follow the same coherence rules as all other types. No tensor-specific exception is allowed.

Rules:

1. `impl Trait for Tensor[...]` is allowed only when orphan rule requirements are satisfied.
2. Blanket tensor impls (for example `impl[T] Trait for Tensor[T]`) are allowed only if:
   1. orphan rule is satisfied,
   2. blanket parameter constraint is satisfied,
   3. overlap/specialization rules are satisfied.
3. Tensor dtype, device, axis names, and runtime shape values MUST NOT be used as additional coherence disambiguators beyond impl header type matching.
4. Performance specialization by dtype/device/backend MUST be expressed in lowering or backend dispatch, not by trait specialization.
5. `impl !Trait for Tensor[...]` remains unsupported in v0.1.


### Trait Default Body Checking

A source trait default body is checked at the trait declaration, independently
of every current or future impl.

Rules:

1. Before checking any default body, implementations MUST resolve and validate
   the trait's associated-type declarations and the complete signatures of all
   required and default methods. Once valid, every trait method signature is
   available in every default body regardless of member source order.
2. A default body's generic well-formedness environment contains the normalized
   bounds from the trait declaration and that method, plus exactly one synthetic
   current-trait obligation. For `trait Rel[A]`, that obligation is the complete
   normalized symbolic obligation `Self: Rel[A]`.
3. The current-trait obligation receives the same finite compiler-known
   prerequisite expansion as an explicit bound. In v0.1 this includes only the
   fixed `Ord -> Eq` edge where applicable; it does not enable user-defined trait
   implication or transitive proof search.
4. The synthetic obligation MAY discharge method lookup, a fully qualified
   current-trait call, or a generic application that requires that exact trait
   reference. `Self::Name` remains the canonical symbolic projection owned by
   the current trait under the associated-type rules above.
5. The obligation MUST NOT discharge a differently instantiated current trait,
   an unrelated trait, or an obligation reached only through recursive impl
   lookup. Those obligations use ordinary lookup and fail with
   `TRAIT_BOUND_NOT_SATISFIED` when unresolved.
6. A default body is type-checked exactly once against abstract `Self`. Its
   validity MUST NOT depend on selecting a concrete impl, on whether an impl
   overrides the method, or on re-checking the body after associated bindings
   are substituted.
7. Calls among required and default methods are checked against the signatures
   collected by Rule 1. Type checking MUST NOT expand, evaluate, or recursively
   re-check a called default body.
8. Directly or mutually recursive default methods therefore do not create trait
   solver recursion. Their effect summaries use the callable call-graph SCC
   procedure in `effects.md`, and their runtime calls have the same possible
   nontermination as ordinary recursive callables.
9. After Rule 1 succeeds and the environment from Rules 2-3 is complete,
   default bodies are checked in source order for primary-diagnostic selection.


### Trait Implementation Method Obligations

Trait method requirements are checked at each `impl Trait for Type` declaration.

Rules:

1. A trait method declaration with no body is a required method.
2. A trait method declaration with a body is a default method and MAY be omitted by an impl.
3. For every required method, the impl MUST declare a method with the same callable base identifier (ignoring a trailing `!`) (`TRAIT_IMPL_MISSING_METHOD` otherwise).
4. If a same-base trait/impl method pair differs only in bang marker, compilation MUST fail with `TRAIT_EFFECT_MISMATCH`.
5. If a default method has no same-base impl method, dispatch uses the default body.
6. If an impl declares a same-base method for a default method, that method MUST match the trait method signature.
7. Trait/impl method signature matching uses:
   1. callable name including `!` bang marker,
   2. generic parameter arity, with generic parameter names alpha-normalized,
   3. order-insensitive normalized bound set for each corresponding generic parameter,
   4. full parameter type list, including receiver parameter type,
   5. return type.
8. Signature matching in Rule 7 occurs after replacing `Self` and normalizing associated-type bindings for the impl.
9. Any signature mismatch other than bang-marker mismatch for a same-base impl method selected to satisfy a trait requirement MUST use `TRAIT_IMPL_SIGNATURE_MISMATCH`.
10. Trait method declarations inherit the visibility of their trait and have no per-item visibility modifier in v0.1.
11. Impl methods satisfy the corresponding trait contract and have no independent visibility modifier. Their callable visibility is the trait method visibility from Rule 10.


### Dispatch Rules

Static dispatch is default.

Dynamic dispatch is not part of the v0.1 baseline runtime model.

1. Trait object usage without explicit `dyn` MUST fail with `DYN_MUST_BE_EXPLICIT`.
2. Explicit `dyn Trait` is parse-valid reserved surface but semantically unsupported in v0.1 non-extern type positions (`DYN_TRAIT_UNSUPPORTED`).
3. v0.1 defines no object-safety rules, trait-object construction/coercion, dynamic method dispatch, or dynamic ownership/effect boundary.

Method lookup for `recv.name(...)` in v0.1:

1. Collect inherent methods declared on the receiver struct with matching method name.
2. If inherent candidates exist, resolve them using overload rules from `modules-resolution.md`:
   1. an inaccessible owner or inherent family: `VISIBILITY_VIOLATION` (no fallback to trait candidates),
   2. one applicable candidate: dispatch to it,
   3. multiple applicable candidates: `OVERLOAD_AMBIGUOUS`,
   4. no applicable candidates in that inherent set: `METHOD_NOT_FOUND` (no fallback to trait candidates).
3. If no inherent candidates exist, collect visible trait methods with matching name and compatible receiver.
   1. Visibility is determined at the call site.
   2. Traits that are not name-resolvable at the call site MUST NOT contribute candidates, even if matching impls exist elsewhere in the program.
4. Resolve trait candidates with the same overload rules:
   1. one applicable candidate: dispatch to it,
   2. multiple applicable candidates: `TRAIT_METHOD_AMBIGUOUS` unless fully qualified syntax is used,
   3. no applicable candidates: continue normal missing-method diagnostics.
5. Trait impl applicability is two-stage:
   1. match the trait and target type structure and infer impl parameters without using impl bounds as a filter,
   2. if exactly one structural impl candidate remains, validate its substituted bounds,
   3. an unsatisfied bound on that unique structural candidate fails with `TRAIT_BOUND_NOT_SATISFIED`, not `METHOD_NOT_FOUND`.
6. A program with multiple structurally matching impls is rejected by coherence before call-site dispatch; call resolution does not use bound success to choose among overlapping impls.

`foo` and `foo!` are distinct method names for lookup and ambiguity checks.

For a receiver whose static type is a bounded generic parameter, methods from its declared visible bounds participate in the trait candidate set. Dispatch remains statically resolved through the concrete impl at instantiation.

Trait re-exports, including prelude re-exports, preserve the target trait and
method identities and participate in visibility exactly as specified by
`modules-resolution.md`. A trait implementation does not create a
concrete-owner callable reference such as `TypeName.method`.

Fully qualified trait methods also follow the ordinary named-callable value
rules:

1. A non-bang `TraitName.method` MAY be coerced to an expected
   `Fn(P0, ..., Pn) -> R` when its declared receiver is the first parameter.
2. The expected receiver component MUST determine one concrete `Self` after
   matching the declared receiver shape (`Self`, `&Self`, or `&mut Self`).
   Associated-type projections in later parameters or the result MUST NOT
   reverse-infer `Self`.
3. After `Self` is known, the compiler MUST select and validate the unique
   applicable impl, normalize associated types, and apply the ordinary callable
   compatibility rules from `calls-references.md`.
4. If the expected `Fn` receiver does not determine one concrete `Self`, the
   reference MUST fail with `TYPE_ARG_INFERENCE_FAILED`. An unsatisfied impl
   obligation uses `TRAIT_BOUND_NOT_SATISFIED`.
5. This coercion preserves the trait method's canonical callable identity. It
   does not create an inherent method or a polymorphic runtime function value.


### Solver Boundary and Cost

The v0.1 trait solver is a finite, first-order nominal obligation resolver.

1. An obligation first matches impl heads by nominal trait and target-type unification without treating bounds as negative filters, then recursively emits only the inline bounds declared by the unique structural match.
2. Implementations MUST memoize normalized obligations or provide an equivalent termination guarantee; revisiting an unresolved obligation cycle MUST NOT count as proof and an otherwise unresolved root obligation MUST fail with `TRAIT_BOUND_NOT_SATISFIED`.
   An early rejection of transformed recursive obligations MUST establish that
   no finite proof exists under the current impl and exact symbolic-assumption
   environment. Growing type arguments, or revisiting the same impl with
   different arguments, do not by themselves establish failure.
3. An exact symbolic obligation in the current declaration's generic well-formedness environment is discharged locally; concrete obligations still require unique impl lookup.
4. Impl lookup MUST be indexed by trait and nominal target head plus a bounded blanket-impl bucket, or provide an equivalent bounded candidate set. It MUST NOT perform unrestricted whole-program proof search per obligation.
5. Projection normalization reuses the same unique-impl lookup and MUST NOT invoke a separate shape, value, effect, or ownership solver.
6. Coherence checking MAY be pairwise within each indexed trait/target-head bucket; its expected worst-case bound is `O(I_b^2)` for `I_b` impl headers in one bucket, not exponential backtracking over bounds.
7. Higher-kinded bounds, supertraits, negative bounds/impls, specialization, associated-type equality reasoning, generic associated types, and dynamic trait objects are outside this solver.
8. `closed trait` limits who may add impls; it does not grant closed-world negation or alter Rules 1-7.
9. The recursive derive eligibility fixed point in `algebraic-data.md` is a
   finite derive-expansion candidate-selection pass, not an ordinary trait proof rule.
   Once selection finishes, only retained generated impls are ordinary lookup
   inputs. The fixed point MUST NOT discharge any other unresolved obligation
   cycle or alter Rules 1-8.


### Effect Marker Stability

A trait method declared with `!` MUST be implemented as effectful.

A trait method declared without `!` MUST NOT be implemented as effectful.

Effect-summary inference follows `effects.md`:

1. Concrete dispatch uses the selected impl method or default-body summary.
2. A non-bang requirement supplies the known pure summary in bounded-generic
   code.
3. A bang requirement supplies `Unknown` in bounded-generic code, even for a
   `closed trait`.
4. Static resolution of the concrete impl at instantiation remains the code
   dispatch rule, but MUST NOT retroactively refine the already checked generic
   callable's v0.1 summary.
5. Trait declarations have no source-level category or state-scope contract in
   v0.1. Such contracts are a future additive extension.


### Required Diagnostics

The following compile-time codes from `../ERRORS.md` are required for this section:

`TRAIT_STATE_FORBIDDEN`, `TRAIT_METHOD_AMBIGUOUS`, `TRAIT_EFFECT_MISMATCH`, `TRAIT_IMPL_MISSING_METHOD`, `TRAIT_IMPL_SIGNATURE_MISMATCH`, `TRAIT_BOUND_INVALID`, `TRAIT_BOUND_NOT_SATISFIED`, `TRAIT_IMPL_ASSOC_TYPE_MISMATCH`, `ASSOCIATED_TYPE_PROJECTION_INVALID`, `ASSOCIATED_TYPE_CYCLE`, `TYPE_ARG_ARITY_MISMATCH`, `CLOSED_TRAIT_IMPL_FORBIDDEN`, `NAME_AMBIGUOUS`, `SELF_TYPE_INVALID`, `VISIBILITY_VIOLATION`, `IMPORT_NOT_FOUND`, `OWNERSHIP_TYPE_POSITION_FORBIDDEN`, `REFERENCE_TYPE_POSITION_FORBIDDEN`, `DYN_MUST_BE_EXPLICIT`, `DYN_TRAIT_UNSUPPORTED`, `ORPHAN_RULE_VIOLATION`, `TRAIT_IMPL_DUPLICATE`, `TRAIT_IMPL_OVERLAP`, `SPECIALIZATION_FORBIDDEN`, `NEGATIVE_IMPL_UNSUPPORTED`, `IMPL_GENERIC_ERROR_UNION_UNSUPPORTED`, `IMPL_UNCONSTRAINED_TYPE_PARAM`, `OVERLOAD_MARKER_REQUIRED`, `OVERLOAD_RETURN_ONLY_DIFFERENCE`, `OVERLOAD_OVERLAP`, `OVERLOAD_AMBIGUOUS`.

These diagnostics apply equally to tensor impl headers (`Tensor[...]`) and non-tensor impl headers.
