# Algebraic Data Types (`enum` and catalog `sum type`)

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Nominal Sum Baseline

v0.1 defines `enum` as the user-facing algebraic data type declaration form.
Standard-library catalogs declare the same source-visible nominal sum ontology
with `sum type`; catalog declaration syntax is defined in
`../stdlib/catalog-signatures.md`.

Canonical declaration form:

```text
enum Name[T, ...] {
  Variant0
  Variant1(T1, T2, ...)
  Variant2 { f1: T1, f2: T2, ... }
}
```

Rules:

1. A source `enum` introduces a nominal type constructor in the type namespace.
2. A catalog `sum type` introduces the same kind of source-visible nominal sum
   owner, although its declaration remains catalog-only.
3. v0.1 variants are:
   1. unit variants (`Variant`),
   2. positional payload variants (`Variant(T1, ..., Tn)`),
   3. named payload variants (`Variant { f1: T1, ..., fn: Tn }`).
4. Variant names within one nominal sum owner MUST be unique
   (`NAME_AMBIGUOUS` otherwise).
5. Named payload field names within one variant MUST be unique
   (`NAME_AMBIGUOUS` otherwise).
6. Each variant introduces a constructor in its owner's namespace:
   1. a unit variant constructor has type `Name[T, ...]`,
   2. a positional variant constructor has type
      `(T1, ..., Tn) -> Name[T, ...]`,
   3. a named payload constructor uses record-initializer surface
      `Name::Variant { f1: e1, ..., fn: en }`.
7. Source enum and catalog-sum constructor and pattern references MUST be
   qualified with the owner path:
   1. constructor forms are `Name::Variant`, `Name::Variant(...)`, and
      `Name::Variant { ... }`,
   2. match pattern forms are `Name::Variant`, `Name::Variant(...)`, and
      `Name::Variant { ... }`,
   3. `Name.Variant` is ordinary member-access syntax and MUST NOT resolve as a
      variant,
   4. unqualified `Variant` references are not part of this nominal-sum surface,
   5. unqualified pattern identifiers that collide with variants of the
      scrutinee's nominal sum type are rejected as `NAME_AMBIGUOUS` rather than
      being silently treated as bindings,
   6. a generic owner uses those same forms without constructor-head type
      arguments,
   7. generic owner arguments are inferred from the selected payload and an
      independently established expected owner type under `type-system.md`; a
      unit variant or a parameter absent from the selected payload therefore
      requires such expected context.
8. Built-in residual ADTs (`Opt`/`Res`) keep their existing unqualified
   constructor surface (`Some`, `None`, `Ok`, `Err`) for v0.x compatibility.
9. Source-enum visibility is enum-level in v0.1:
   1. variant-level visibility modifiers are not part of baseline syntax,
   2. attempting variant-level visibility (for example `pub Variant`) MUST fail with `ENUM_VARIANT_VISIBILITY_FORBIDDEN`,
   3. variant constructors share visibility with the enclosing enum declaration.
10. `pub enum` exports the enum type and its variants through the owner namespace
    (`Name::Variant`); private enum declarations keep both private to module
    scope. A visible catalog sum exports only its owner as an importable symbol;
    its variants are reached through that owner.
11. After a constructor head resolves uniquely, its normalized semantic target
    kind MUST match the written surface: a struct literal requires `struct`, and
    a variant constructor requires `variant_constructor`. A mismatch MUST fail
    with `CONSTRUCTOR_TARGET_KIND_MISMATCH` before payload validation.
12. After target-kind validation succeeds, the written payload surface MUST
    match the variant's declared form:
    1. bare surface selects `unit`,
    2. parentheses select `positional`, including empty parentheses,
    3. braces select `named`, including empty braces,
    4. a mismatch MUST fail with `CONSTRUCTOR_PAYLOAD_FORM_MISMATCH`,
    5. this check precedes payload arity, named-field validation, payload
       expression typing, and generic owner inference.
13. Once the form matches, named-payload constructors reuse
    record-initializer diagnostics:
   1. unknown field key -> `STRUCT_INIT_UNKNOWN_FIELD`,
   2. duplicate field key -> `STRUCT_INIT_DUPLICATE_FIELD`,
   3. missing declared field -> `STRUCT_INIT_MISSING_FIELD`.
14. Pattern validation does not reuse constructor diagnostics. Unit,
    positional, and named-payload variants retain distinct pattern forms, and
    their structural failures use the `PATTERN_*` diagnostics and validation
    order defined in `control-flow.md`.
15. A source `enum` MAY declare zero variants. Such a declaration introduces no
    constructors and is `Uninhabited` under `type-system.md`; it remains a
    distinct nominal type and is not canonical `Never`.

### Attribute Target Applicability

Attribute parsing is intentionally broader than v0.1 semantic support. Semantic
validation uses one default target rule before applying each attribute family's
argument and eligibility rules.

Rules:

1. An attribute path outside the supported v0.1 families MUST fail with
   `ATTRIBUTE_UNSUPPORTED`, regardless of its parse-valid target.
2. After an attribute is recognized as a supported family, its target MUST
   belong to that family's closed semantic target set defined below.
3. A supported attribute on any other parse-valid target MUST fail with
   `ATTRIBUTE_TARGET_UNSUPPORTED`, unless the attribute-specific rules assign a
   more specific target diagnostic.
4. The v0.1 attribute-specific target diagnostics take precedence over the
   default:
   1. invalid `derive` targets use `DERIVE_TARGET_UNSUPPORTED`,
   2. invalid `test` targets use `TEST_ATTRIBUTE_TARGET_UNSUPPORTED`,
   3. `ignore` without a same-item `test` uses
      `TEST_IGNORE_REQUIRES_TEST`.
5. `default` is semantically supported only on an enum variant. On any other
   parse-valid target it MUST fail with `ATTRIBUTE_TARGET_UNSUPPORTED`.
6. Once a target is accepted, malformed arguments, duplicate entries, provider
   resolution, and derive eligibility use their attribute-specific diagnostics;
   they MUST NOT be reclassified as target failures.

### Derive Attribute Semantics

v0.1 uses attribute-lite with explicit derive semantics on data declarations.

Rules:

1. `#[derive(...)]`, enum-variant `#[default]`, and function-level `#[test]`/`#[ignore]` are the only attribute families with normative semantics in v0.1.
2. Other attribute names are parse-valid but MUST fail semantic validation with `ATTRIBUTE_UNSUPPORTED`.
3. `derive` semantic targets in v0.1 are:
   1. `struct`,
   2. `enum`.
4. Applying `derive` to other parse-valid targets (for example `fn`, `impl`) MUST fail with `DERIVE_TARGET_UNSUPPORTED`.
5. Each `derive` entry is interpreted as a trait path.
6. Repeating the same normalized trait path in one `derive(...)` list MUST fail with `DERIVE_DUPLICATE_ENTRY`.
7. Each derive trait path MUST resolve to a visible trait symbol at the annotated item site; failure MUST use `DERIVE_TRAIT_RESOLUTION_FAILED`.
8. A resolved trait path with no active derive support/provider MUST fail with `DERIVE_TRAIT_UNSUPPORTED`.
9. Derive expansion is constrained:
   1. expansion MAY generate only `impl Trait for Type` declarations for the annotated type,
   2. expansion MUST NOT introduce new free functions, new nominal types, new constants, or new modules,
   3. when an executable custom-provider profile is active, violating this expansion boundary MUST fail with `DERIVE_EXPANSION_FORBIDDEN`,
   4. in v0.1 locked baseline, this violation path is non-baseline and excluded from required conformance.
10. Retained expanded impls participate in ordinary coherence checks (`duplicate`, `overlap`, `orphan`, `specialization`) with no derive-specific exception.
11. If user-written impl and derived impl target the same `(Trait, Type)` pair, compilation MUST fail with `TRAIT_IMPL_DUPLICATE` under the existing duplicate-impl rule.
12. Derive evaluation MUST be deterministic and side-effect free in v0.1:
   1. no file/network/process/clock/randomness access,
   2. same input program yields the same expanded impl set.
13. Provider-specific payload constraints (for example required field trait bounds) are allowed, but failure MUST use `DERIVE_ELIGIBILITY_FAILED`.

### Test Function Attributes

v0.1 defines a minimal test-attribute surface for top-level functions.

Rules:

1. `#[test]` target:
   1. MUST be a top-level function declaration.
   2. Applying `#[test]` to non-function or non-top-level function targets MUST fail with `TEST_ATTRIBUTE_TARGET_UNSUPPORTED`.
2. `#[test]` function signature:
   1. MUST have zero parameters.
   2. MUST NOT declare generic parameters.
   3. return type MUST be either `Unit` or `Res[Unit, E]` for some error type `E`.
   4. Signature violations MUST fail with `TEST_FUNCTION_INVALID_SIGNATURE`.
   5. `Never` is not an additional test result protocol. A test declared
      `-> Never` fails Rule 2.3, although a valid `-> Unit` test body may itself
      have no normal continuation under `type-system.md`.
3. `#[ignore]` target:
   1. MUST be attached to a top-level function that also has `#[test]` on the same declaration.
   2. Using `#[ignore]` without `#[test]` MUST fail with `TEST_IGNORE_REQUIRES_TEST`.
4. Effect semantics for test functions are unchanged from core language rules:
   1. non-bang test functions MUST remain non-effectful except permitted local-state-only mutation paths,
   2. effectful test functions MUST be declared as bang (`fn ...!`).
5. v0.1 only standardizes `#[test]` and `#[ignore]`; additional test attributes are outside the baseline.
6. Their stabilization is tracked by `LANG-OI-018` in `../open-items/language.md`, with candidate designs in `../../roadmap/language/test-attributes.md`.

### Custom Derive Provider Model

v0.1 baseline keeps derive-provider execution in `locked` mode.

Rules:

1. A derive trait path MAY be backed by:
   1. built-in provider,
   2. implementation-trusted provider.
2. External user-provided provider execution is not part of v0.1 conformance baseline.
3. v0.1 baseline active provider set is closed:
   1. source-level registration is not part of baseline surface,
   2. manifest-level provider registration is not part of baseline surface,
   3. implementation-trusted provider availability is profile/toolchain fixed input.
4. If a derive entry resolves to a visible trait with no active provider in the closed set, compilation MUST fail with `DERIVE_TRAIT_UNSUPPORTED`.
5. Provider-set construction point is fixed for deterministic compilation:
   1. provider set is constructed once at compile setup,
   2. provider set construction completes before derive expansion and before full type checking.
6. Derive expansion phase ordering is fixed:
   1. module canonical-path order,
   2. declaration source order within module,
   3. derive-entry order inside one `derive(...)` list.
7. Provider evaluation MUST be pure and deterministic:
   1. no file/network/process/time/random access,
   2. same input program and same active provider set MUST yield identical expanded impl set.
8. Provider output boundary is strict:
   1. only `impl Trait for AnnotatedType` declarations MAY be emitted,
   2. provider MUST NOT emit free functions, new nominal types, consts, modules, or impls for non-annotated receiver types.
9. Retained provider-produced impls participate in ordinary coherence/orphan/overlap checks with no exemptions.
10. When an executable custom-provider profile is active, provider expansion violating Rules 7-8 MUST fail with `DERIVE_EXPANSION_FORBIDDEN`.
11. External-provider registration and expansion are tracked by `LANG-OI-019`,
    while executable-provider authorization is tracked by `LANG-OI-020`, in
    `../open-items/language.md`. Candidate Slot A designs live in
    `../../roadmap/language/attributes.md`; none are required for v0.1
    conformance.
12. v0.1 locked baseline excludes `DERIVE_EXPANSION_FORBIDDEN` from required conformance suites.

### Built-in Derive Allowlist

v0.1 defines a narrow structural built-in derive trait allowlist:

1. `Debug`
2. `Clone`
3. `Eq`
4. `Hash`
5. `Ord`
6. `Default`

Rules:

1. A derive entry resolving to one of the built-in allowlist traits MUST be accepted when that trait's eligibility constraints are satisfied.
2. A derive entry resolving to a visible trait not in the active built-in allowlist and without an active custom provider MUST fail with `DERIVE_TRAIT_UNSUPPORTED`.
3. `Display`, `From`, `TryFrom`, `Into`, `TryInto`, and `Error` are not part of the v0.1 built-in derive surface.
4. v0.1 MUST NOT auto-generate conversion impls (`From`, `TryFrom`, `Into`, or `TryInto`) from any built-in derive.
5. A prelude trait being visible does not imply built-in derive support; if no implementation-trusted provider is active for that trait, deriving it MUST fail with `DERIVE_TRAIT_UNSUPPORTED`.

### Built-in Derive Eligibility and Generated Contracts

Built-in derives generate canonical impls for the annotated receiver type and MUST obey these eligibility and behavior rules.

General rules:

1. Eligibility failures for built-in derives MUST use `DERIVE_ELIGIBILITY_FAILED`.
2. When `DERIVE_ELIGIBILITY_FAILED` is emitted for built-in derives, `reason` SHOULD be one of:
   1. `missing_required_bound`,
   2. `missing_required_eq`,
   3. `enum_default_missing`,
   4. `enum_default_multiple`,
   5. `enum_default_non_unit`,
   6. `recursive_type_argument_growth`.
3. A participating payload field is any declared struct field or enum variant payload field whose value is read, cloned, formatted, compared, hashed, or default-constructed by the generated impl.
4. Built-in derives MUST NOT synthesize hidden generic bounds for source type parameters.
5. An explicit inline bound on a source type parameter MAY satisfy the requested derive's trait requirement.
6. If a participating payload field type contains a type parameter without the required explicit bound in any position that requires trait support for the requested derive, the derive MUST fail with `DERIVE_ELIGIBILITY_FAILED` (`reason = missing_required_bound`).
7. Generic parameters that do not appear in any participating payload field do not affect built-in derive eligibility.
8. A generated impl MUST preserve the annotated type's declared generic parameters and bounds and MUST NOT add provider-inferred bounds that are absent from source.

#### Recursive derive eligibility fixed point

Recursive and mutually recursive declarations use a derive-local candidate
fixed point. This fixed point decides only whether generated impl candidates
exist; it does not make an unresolved ordinary trait obligation cycle count as
proof.

Rules:

1. After active providers have produced their finite impl-candidate summaries,
   the compiler MUST collect the complete candidate set for the current package
   or file-set build before testing recursive eligibility. The dependency lookup
   key is the complete normalized trait reference plus the annotated nominal
   declaration head. Each provider-output source occurrence remains a distinct
   candidate; an implementation MAY share eligibility analysis for equal keys
   but MUST preserve their multiplicity for ordinary duplicate-impl checking.
2. Imported dependency impls and explicit source impls are established ordinary
   lookup inputs, not provisional derive candidates. A provider predicate that
   is not a positive trait-support requirement is evaluated directly and cannot
   become true merely through a candidate cycle.
3. For each candidate, the compiler MUST collect its finite positive
   trait-support requirements after ordinary alias expansion and projection
   normalization. Requirement resolution follows the ordinary trait solver,
   except that reaching a provisional generated candidate records a dependency
   edge instead of recursively proving that candidate.
4. A dependency edge MUST retain the normalized substitution from the source
   candidate's generic parameters to the target candidate's receiver arguments.
   The substituted explicit bounds declared by the target candidate MUST hold
   in the source candidate's generic environment; derive MUST NOT infer or add a
   missing bound.
5. A normalization error, ambiguous or missing established impl, unsatisfied
   explicit bound, provider rejection, or ordinary obligation cycle that does
   not reach a provisional candidate is a direct failure of the requiring
   candidate.
6. Candidate eligibility is the greatest fixed point over this finite positive
   dependency graph:
   1. initially treat every candidate without a direct failure as provisional,
   2. remove every candidate that depends on a removed or directly failed
      candidate,
   3. repeat Rule 6.2 until no candidate changes,
   4. retain every remaining candidate, including a positive self-recursive or
      mutually recursive strongly connected component.
7. Each dependency edge inside a cyclic candidate component MUST be
   non-growing. After normalization, every occurrence of a generic parameter
   introduced by the source candidate MUST be one complete target receiver
   argument. Source parameters MAY be reordered, repeated, or omitted, and a
   target argument containing no source parameter MAY be any closed type.
8. A recursive target argument is growing when a source parameter occurs below
   any type-forming node, including a nominal or generic application, tuple,
   reference, callable, or remaining associated-type projection. A growing edge
   MUST fail with `DERIVE_ELIGIBILITY_FAILED`
   (`reason = recursive_type_argument_growth`).
9. Rule 7 applies only to edges in a cyclic component. An acyclic dependency may
   use an ordinary nested type argument and is checked by the established impl
   and explicit-bound rules.
10. The non-growing test is deliberately a conservative syntactic bound. It
    need not accept a cycle whose composed substitutions happen to produce a
    finite orbit if one constituent edge is growing.
11. Fixed-point membership MUST be independent of provider traversal, module
    enumeration, derive-list, graph traversal, and SCC discovery order.
    Diagnostics retain the canonical derive expansion and source ordering
    defined above; SCC numbering and traversal order are not observable.
12. Only retained candidates enter ordinary coherence checking. Retention is
    not coherence approval and grants no exception to duplicate, overlap,
    orphan, or specialization rules.
13. Candidate solving MUST operate on finite declaration/provider summaries. It
    MUST NOT enumerate concrete type-argument or monomorphized-instance orbits.

Examples of the recursive substitution boundary:

```text
#[derive(Eq)]
struct Tree[T: Eq] {
  field children: Seq[Tree[T]]       // accepted: T -> T
}

#[derive(Eq)]
struct Flip[A: Eq, B: Eq] {
  field children: Seq[Flip[B, A]]    // accepted: (A, B) -> (B, A)
}

#[derive(Eq)]
struct Grow[T: Eq] {
  field children: Seq[Grow[Opt[T]]]  // rejected: T occurs below Opt
}
```

`Debug` derive:

1. `Debug` derive is eligible only when every participating payload field type supports `Debug`.
2. The generated debug representation MUST be deterministic and MUST preserve declaration order.
3. Struct debug output MUST use:
   1. `TypeName {}` for zero-field structs,
   2. `TypeName { f0: v0, f1: v1 }` for non-empty structs, where `fN` is the field name and `vN` is that field's `Debug` representation.
4. Enum debug output MUST use:
   1. `EnumName::Variant` for unit variants,
   2. `EnumName::Variant(v0, v1)` for positional payload variants,
   3. `EnumName::Variant { f0: v0, f1: v1 }` for named payload variants.
5. No extra whitespace is permitted beyond the spaces shown in Rules 3-4.

`Clone` derive:

1. `Clone` derive is eligible only when every participating payload field type supports `Clone`.
2. The generated impl MUST provide the prelude `Clone.clone(self: &Self) -> Self` method.
3. The generated clone operation MUST be pure and non-consuming: cloning through `&Self` MUST NOT move from or mutate the receiver.
4. For `struct`, the generated clone value MUST have the same nominal type and field values obtained by cloning each field in declaration order.
5. For `enum`, the generated clone value MUST preserve the selected variant and clone each payload field in declaration order.

Comparison and hash derives:

1. For `Eq`, `Ord`, and `Hash`, generated impls MUST be field/variant-structure coherent and deterministic.
2. A generated `Eq` impl MUST provide the exact prelude method `fn Eq.eq(self: &Self, other: &Self) -> Bool`.
3. `Eq` for structs MUST compare all participating fields in declaration order and short-circuit at the first unequal pair; values are equal iff every participating field is equal.
4. `Eq` for enums MUST compare variant identity before payloads; values with different variants are not equal, and values with the same variant compare payload fields in declaration order and short-circuit at the first unequal pair.
5. A generated `Ord` impl MUST provide the exact prelude method `fn Ord.cmp(self: &Self, other: &Self) -> Ordering`.
6. `Ord` for structs MUST compare participating fields in declaration order with `Ord.cmp`, return the first non-`Equal` result, and return `Equal` when every field comparison is `Equal`.
7. `Ord` for enums MUST compare variant declaration order first, then compare payload fields under Rule 6 when variants match.
8. `Ord` derive is eligible only when every participating payload field type implements `Ord` and the annotated type implements `Eq`.
9. The complete built-in derive expansion set is considered before the Rule 8 `Eq` prerequisite is checked, so `#[derive(Eq, Ord)]` and `#[derive(Ord, Eq)]` are equivalent. An applicable explicit `Eq` impl also satisfies the prerequisite.
10. Missing `Eq` for Rule 8 MUST fail with `DERIVE_ELIGIBILITY_FAILED` (`reason = missing_required_eq`).
11. A generated `Hash` impl MUST provide the exact prelude method `fn Hash.hash(self: &Self, seed: u64) -> u64`.
12. `Hash` derive is eligible iff every participating payload field type implements `Hash`; the annotated type need not implement `Eq`.
13. `Hash` for structs MUST start with the caller-supplied seed and hash participating fields in declaration order, passing each result as the next field's seed. A zero-field struct returns the supplied seed.
14. `Hash` for enums MUST first hash the zero-based declaration ordinal of the selected variant as a `u64`, then hash payload fields in declaration order by the same seeded fold. This variant tag prevents equal payload structures in distinct variants from being required to collide.
15. Derived hash values are deterministic only within the hash determinism context defined by `../stdlib/prelude.md`; exact numeric outputs are not portable across contexts.
16. If any participating payload field type does not satisfy required trait constraints for the requested derive, derive MUST fail with `DERIVE_ELIGIBILITY_FAILED` (`reason = missing_required_bound`).
17. For floating payloads (`f16`, `bf16`, `f32`, `f64`), generated `Eq`/`Ord`/`Hash` semantics MUST use the unified floating total-order policy from `../stdlib/conventions.md`, including:
   1. NaN-last ordering relative to non-NaN values,
   2. NaN payload/sign distinction,
   3. signed-zero distinction (`-0.0` vs `+0.0`),
   4. hash/equality coherence.

`Default` derive:

1. For `struct`, derive is eligible only when every declared field type supports `Default`; generated default value is constructed by per-field `Default` in declaration order.
2. For `enum`, derive is eligible only when exactly one variant is marked with `#[default]`.
3. For `enum`, the `#[default]` variant MUST be a unit variant.
4. Violations of Rules 2-3 MUST fail with `DERIVE_ELIGIBILITY_FAILED` using:
   1. `reason = enum_default_missing` when no `#[default]` variant exists,
   2. `reason = enum_default_multiple` when more than one `#[default]` variant exists,
   3. `reason = enum_default_non_unit` when marked variant is positional or named-payload.
5. The generated default value for an enum is the selected `#[default]` unit variant.
6. `#[default]` has no semantic effect outside `derive(Default)` eligibility in v0.1.
7. v0.1 does not expose a source-callable `Default.default()` associated method; associated/static trait methods remain outside the v0.1 baseline.

### Recursive Value Layout and `Box`

`Box[T]` is the v0.1 explicit indirection type for recursive value layout.
It is language-intrinsic, re-exported by `std::prelude`, and follows ordinary
ownership-free value semantics.

Inline-layout rules:

1. Before layout analysis, the compiler MUST complete built-in-name reservation, generic arity checking, transparent-alias cycle detection/expansion, and ordinary type-position validation, in that order.
2. Layout analysis MUST use substitution-aware inline expansion over user `struct` and `enum` storage declarations.
3. Generic applications are not monomorphized layout-graph nodes:
   1. actual type arguments are substituted while following an inline storage path,
   2. cycle identity is the user nominal declaration head, independent of its actual arguments,
   3. encountering a nominal head already active on the same inline expansion path is a cycle, including transformed recursion such as `Grow[T] -> Grow[Opt[T]]`,
   4. substitutions from distinct generic application paths MUST NOT be merged in a way that creates a cycle absent from either concrete path.
4. Associated-type projections in storage positions participate in the same finite analysis:
   1. a projection whose base substitution selects one impl is normalized through that impl's binding before inline traversal continues,
   2. an unresolved generic projection uses the same canonical identity as ordinary type checking: base-parameter identity, complete normalized declaring-trait reference including type arguments, and associated name,
   3. impl binding summaries retain that identity plus the current substitution-path context; edges from distinct bases or generic application paths MUST NOT be merged,
   4. impl binding summaries provide the finite substitution edges for those symbolic projections,
   5. a concrete layout path that normalizes back to an active nominal declaration head is a recursive layout cycle.
5. Implementations MUST use a finite declaration/impl summary, occurs-check, or equivalent algorithm and MUST NOT enumerate an unbounded sequence of instantiated types.
6. Inline traversal continues through tuples, `Opt`, `Res`, associated projections, and user-defined nominal wrappers.
7. Inline traversal stops at:
   1. `Box[...]`,
   2. references,
   3. raw pointers in their allowed extern positions,
   4. callable type `Fn(...) -> ...`, whose parameter/result types are signature metadata rather than inline closure storage,
   5. opaque task completion value `Task[...]`, whose result type is not stored inline in the value,
   6. recognized runtime-sized storage families `Seq`, `Vec`, `Map`, `Set`, `Tensor`, `PVec`, `PMap`, and `PSet`.
8. Every inline expansion path that repeats a nominal head, including a path mediated by an associated projection, MUST fail with `TYPE_RECURSIVE_LAYOUT_REQUIRES_INDIRECTION`.
9. The primary cycle edge is the earliest source-positioned field or variant-payload type occurrence that belongs to any invalid cycle; ties use owner declaration position, then target nominal declaration position.
10. The primary diagnostic span MUST identify the selected storage occurrence.
    When related text is emitted, it SHOULD identify a deterministic cycle path
    beginning with that edge using `Owner -> Target` edge notation. Diagnostic
    emission ordering is applied afterward under `../errors/ordering.md`.
11. A recursive declaration is valid when every path back to an active nominal head crosses a recognized indirection boundary.
12. A stop boundary does not make an otherwise invalid type position legal; earlier diagnostics from Rule 1 take precedence.
13. `Own[...]` is not a layout-indirection boundary in v0.1. Its use in a user field or enum payload fails first with `OWNERSHIP_TYPE_POSITION_FORBIDDEN`.
14. Recursion checks are compile-time checks and MUST NOT be deferred to runtime diagnostics.
15. `Never` is a terminal leaf for recursive-edge discovery and contributes no
    nominal expansion edge. This rule does not assign it a size, alignment,
    representation, niche, or field-layout guarantee.

Examples:

```text
enum BadList {
  Cons(i64, BadList)  # TYPE_RECURSIVE_LAYOUT_REQUIRES_INDIRECTION
  Nil
}

enum List {
  Cons(i64, Box[List])
  Nil
}
```

Pattern bindings do not consume ordinary ADT payloads.
The only ownership-bearing ADTs in v0.1 are the built-in conditional carriers;
their consuming match form is `match move r` as defined in `ownership-boundary.md`.

### Built-in ADT Reservation

`Opt`, `Res`, ownership constructor `Own`, layout constructor `Box`, callable
head `Fn`, structural error-union head `Union`, and intrinsic bottom type
`Never` remain built-in in v0.x.

Rules:

1. The type-namespace names `Opt`, `Res`, `Own`, `Box`, `Fn`, `Union`, and
   `Never` are reserved.
2. A user `struct`, `enum`, `trait`, type alias, generic type parameter, or type-namespace import alias MUST NOT introduce a Rule 1 name (`RESERVED_BUILTIN_NAME` otherwise).
3. The value-constructor names `Some`, `None`, `Ok`, and `Err` are reserved; user declarations MUST NOT introduce them as value constructors in v0.x (`RESERVED_BUILTIN_NAME` otherwise).
4. Reservation validation precedes declaration-body, generic-arity-at-use, and recursive-layout validation for the invalid declaration.
5. `result {}` and `?` semantics are defined only for built-in `Opt`/`Res` families in v0.x.
6. Generalized user-defined residual algebra is out of scope for v0.x.

### Layout and ABI Status

Enum value layout/ABI is intentionally unspecified in v0.x baseline.

Rules:

1. Implementations MAY choose layout strategy for enums.
2. Conformance MUST NOT require cross-implementation enum ABI compatibility in v0.x.
3. Enum values have no baseline FFI representation contract in v0.1:
   1. user-defined `enum` types MUST NOT appear in v0.1 extern parameter/return signatures,
   2. enum-pointer extern signatures are also outside v0.1 subset.
4. Opt-in representation attributes (for example `repr(...)`) are deferred by
   `LANG-OI-016` in `../open-items/language.md`.
5. `Never` and nominal types made structurally uninhabited by a `Never` field or
   impossible variant set have no source-visible size, alignment, niche, or ABI
   guarantee in v0.1.
