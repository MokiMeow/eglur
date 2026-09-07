# Modules and Name Resolution

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Execution Unit

A compilation unit SHALL be a single source file.
Package-mode and explicit file-set builds MAY compile multiple compilation units as one build input.
This file uses `compilation unit` for the per-source-file syntax/module unit and `file set` or `package` for multi-file build inputs.

Module-path canonicalization has two baseline modes:

1. Package mode:
   1. applies when compilation is driven by package model from `package.md`,
   2. source root is fixed to `src/`,
   3. canonical module path is computed from `src/`-relative path components
      (excluding extension), joined by `::`, and
   4. canonical module identity is `(PackageInstanceId, ModulePath)` under the
      package-identity contract in `package.md`.
2. File-set mode:
   1. applies to explicit virtual-file compilation outside the package model,
   2. canonical module paths follow the file-set invocation contract below.

Baseline source extension policy:

1. v0.1 canonical source extension is `.eglur`.
2. Package-mode canonicalization examples and required conformance fixtures MUST use `.eglur`.
3. Implementations MAY accept additional extensions as non-baseline tooling extensions, but this MUST NOT change canonical-path semantics for `.eglur` files.

Example: `src/net/http.eglur` maps to canonical module `net::http`.

### Source-Addressable Canonical Module Paths

Canonical module paths are derived from source location, but every resulting
segment must also be writable in source without a second spelling scheme.
Within package mode, the path alone is not a globally unique module identity;
the owning `PackageInstanceId` is also required.

For this section:

1. the source-relative path is the structurally valid package `src/`-relative
   path or file-set logical-root-relative path,
2. a derived component spelling is each directory component unchanged and the
   final filename component with its exact `.eglur` suffix removed,
3. a canonical module segment and its source spelling are the same sequence of
   Unicode scalar values as that derived component spelling.

Rules:

1. Every derived component spelling of a user-supplied `.eglur` source MUST be
   accepted as one ordinary source `Identifier` in the corresponding
   `ModulePath` position under `syntax.md`.
2. Consequently, every component MUST satisfy the ASCII base identifier grammar
   `[A-Za-z_][A-Za-z0-9_]*`, MUST NOT be the lone `_`, and MUST NOT be a reserved
   source keyword.
3. The first component additionally MUST NOT be `self` or `super`, because those
   spellings select relative-import authority at the start of an `ImportPath`.
   Either spelling MAY occur in a later component where `syntax.md` treats it as
   an ordinary identifier.
4. A contextual spelling is valid wherever the source grammar treats it as an
   ordinary identifier in that path position. In particular, contextual
   spellings MUST NOT be rejected merely because they have a special meaning in
   another syntactic position.
5. Implementations MUST copy every valid component scalar-for-scalar into the
   canonical module path. They MUST NOT case-fold, apply Unicode normalization,
   replace punctuation, escape a component, or otherwise mangle it into a source
   identifier.
6. A violation of Rules 1-3 MUST fail with `MODULE_PATH_COMPONENT_INVALID` during
   resolve-phase build-input validation.
7. Validation order is normative:
   1. file-set structural validation under the invocation contract below, or
      package root, manifest, dependency-graph, and lockfile validation under
      `package.md`, MUST complete first,
   2. reserved `std` root validation under the next section MUST precede generic
      component validation, even when a later component is invalid,
   3. generic component validation MUST precede source decoding, lexing, parsing,
      optional `module` header verification, import resolution, and program-entry
      validation.
8. When multiple source units in the same package source tree or file set
   violate Rules 1-3, the unit whose complete source-relative path is first under
   the [canonical lexical text order](invariants.md#canonical-lexical-text-order)
   MUST determine the primary diagnostic. Package paths are compared relative to
   `src/`; file-set paths are compared relative to the logical root. The
   comparison includes the `.eglur` extension and is independent of filesystem
   or mapping enumeration order.
9. Within the selected source path, the leftmost invalid component determines
   diagnostic data. `name` MUST contain its exact derived component spelling and
   `index` MUST contain its zero-based component position.
10. The primary span MUST be the zero-width physical byte range `0..0` in the
    selected source unit, including for an empty or undecodable unit. For valid
    UTF-8, its text projection begins and ends at line 1, column 1; for an
    undecodable unit, the span MUST omit `text`. A `module` verification header,
    when present, does not own this failure.
11. These rules apply only to `.eglur` source units. Non-source support files in
    a file set or package do not define canonical modules and are not subject to
    component validation.

### Reserved Standard-Library Root

The exact lowercase first segment `std` is the implementation-owned
standard-library module root in both package and file-set modes.

Rules:

1. A user-supplied `.eglur` source MUST NOT have a derived canonical module
   path whose first segment is `std`.
2. Rule 1 applies to every source unit in a user package or file-set build
   input. It does not apply to implementation-owned standard-library source.
3. A Rule 1 violation MUST fail during resolve-phase build-input validation
   with `MODULE_ROOT_RESERVED`.
4. Validation order is normative:
   1. file-set structural validation under the invocation contract below, or
      package root, manifest, dependency-graph, and lockfile validation under
      `package.md`, MUST complete first,
   2. reserved-root validation MUST then precede generic source-addressability
      validation above, source decoding, lexing, parsing, optional `module`
      header verification, import resolution, and program-entry validation.
5. Reserved-root validation examines the first derived component directly and
   does not depend on whether later components are otherwise source-addressable.
6. When multiple source units in the same package source tree or file set
   violate Rule 1, the unit whose complete canonical source-relative path is
   first under the
   [canonical lexical text order](invariants.md#canonical-lexical-text-order)
   MUST determine the primary diagnostic. Package paths are compared relative
   to `src/`; file-set paths are compared relative to the logical root. The
   comparison includes the `.eglur` extension and is independent of filesystem
   or mapping enumeration order.
7. The primary span MUST be the zero-width physical byte range `0..0` in the
   selected source unit, including for an empty or undecodable unit. For valid
   UTF-8, its text projection begins and ends at line 1, column 1; for an
   undecodable unit, the span MUST omit `text`. A `module` verification header,
   when present, does not own this failure. Diagnostic data MUST include `name`
   with exact value `std`.

### File-set Invocation Contract

A file-set build request supplies a virtual mapping from relative POSIX paths to file contents and designates one mapped source as its entry.
For an executable invocation, that mapped source is also the entry source used
to select `main!` under `program-model.md`.

File-set root rules (v0.1 baseline):

1. Every mapped path MUST be a canonical relative POSIX path:
   1. it MUST NOT be absolute,
   2. it MUST NOT contain an empty, `.` or `..` component,
   3. it MUST NOT contain a backslash.
2. A file set containing exactly one `.eglur` source has implicit logical root `.` in the request path namespace and MUST NOT provide an explicit logical root.
3. A file set containing two or more `.eglur` sources MUST provide an explicit logical root.
4. An explicit logical root MUST be either `.` or a canonical relative POSIX path under Rule 1.
5. Every `.eglur` source in the request MUST be located at or below the logical root.
   Non-source support files MAY be outside it.
6. The canonical module path of each source is its logical-root-relative path with the `.eglur` extension removed and path components joined by `::`.
7. The invoker MUST NOT infer or revise the logical root from:
   1. the longest common ancestor of source paths,
   2. the entry path,
   3. `module` verification headers.
8. A missing required root, a non-canonical root or source path, or a source outside the root is invalid invocation configuration and MUST be rejected before source compilation.
   These failures do not produce a source compile diagnostic because no source construct owns the error.

The optional `module <path>` declaration is a verification header, not a renaming mechanism:

1. It MAY appear once at most.
2. It MUST appear before all non-empty top-level items.
3. If present, declared `<path>` MUST equal canonical module path, else compilation MUST fail with `MODULE_PATH_MISMATCH`.

### Top-level Constants

`const` introduces a module-scope compile-time constant binding.

Rules:

1. `const` declarations are only valid at top-level in v0.1.
2. `const` declarations MUST include explicit type annotation (`const NAME: Type = expr`).
3. `const` initializer expressions MUST be const-evaluable under `validation-matrix.md`.
4. `pub const` exports a constant to other modules.
5. `const` initializers MAY reference other constants.
6. Constant dependency graphs MUST be acyclic; dependency cycles (including cross-module cycles) MUST fail with `CONST_CYCLE`.
7. If dependency graph is acyclic, same-module references to constants declared later in source order MUST fail with `CONST_FORWARD_REFERENCE`.
8. When both cycle and forward-reference conditions would apply to the same declaration set, `CONST_CYCLE` MUST take precedence.

### Imports

`import` performs compile-time name binding only and MUST NOT introduce runtime side effects.

v0.1 import forms:

1. `import a::b::name`
2. `import a::b::name as alias`
3. `import a::b::{x, y as z, f! as g!}`
4. `import self::sub::name`
5. `import super::peer::name`
6. `pub import ...` (re-export from current module)

Rules:

1. `self` resolves to current module.
2. `super` resolves to parent module and MUST fail at module root (`IMPORT_SUPER_AT_ROOT`).
3. Unresolved module path or symbol MUST fail with `IMPORT_NOT_FOUND`.
4. If imported symbol is bang (`name!`), alias MUST also be bang (`alias!`) or compilation MUST fail with `IMPORT_BANG_ALIAS_MUST_BANG`.
5. Wildcard imports are not part of v0.1.
6. An import path whose first segment is `std` resolves exclusively against the
   implementation-owned standard-library namespace. Resolution MUST NOT fall
   back to a local or dependency module; an unresolved standard-library module
   or symbol MUST fail with `IMPORT_NOT_FOUND`.
7. In package mode, import paths whose first segment equals a validated direct
   dependency import-root key from `package.md` resolve against that dependency
   package's `PackageInstanceId`. Resolution uses the exact decoded key and MUST
   NOT normalize or mangle either spelling.
8. The direct dependency key set for Rule 7 is taken from the owning node's
   dependency table in validated `Eglur.lock` (`package.md`) and MUST be
   deterministic for fixed package inputs.
9. If lockfile validation fails, module resolution MUST NOT proceed and compile failure MUST use lockfile diagnostics from `package.md`.
10. In package mode, import paths whose first segment attempts to use a
    transitive-only dependency key as an import root MUST fail with
    `IMPORT_NOT_FOUND`.
11. In package mode, if first-segment lookup is ambiguous between a direct dependency key and a same-root local module path, compilation MUST fail with `IMPORT_ROOT_CONFLICT`.
12. An import or public re-export introduces a local spelling but preserves the
    package-scoped canonical identity and namespace of its target declaration.
    An `as` alias changes only that local spelling.
13. Two declarations with the same module path, namespace, name, and signature
    in distinct package instances remain distinct declarations and, for nominal
    declarations, distinct types. `PackageName` equality MUST NOT merge them.
14. A downstream package MAY resolve a public re-export through one of its
    direct dependency roots even when the re-export's canonical target belongs
    to a transitive-only package. That lookup targets the direct dependency's
    public module interface, not the transitive package's import root, and
    therefore does not violate Rule 10. The re-export MUST preserve the target's
    canonical package-scoped identity and MUST NOT add the transitive package's
    dependency key to the downstream package's import-root set.

### Callable Effect Metadata

Effect-summary semantics and inference are owned by `effects.md`. Module
resolution preserves those facts across compilation-unit and package
boundaries.

Rules:

1. Resolved metadata for every callable MUST include its bangness and
   `EffectSummary`.
2. A compilation artifact or module interface that exposes a callable MUST
   carry those facts in a deterministic form sufficient to reconstruct the
   lattice value defined in `effects.md`.
   In package mode, loading that metadata MUST bind the declaration to the
   current resolved graph's owning `PackageInstanceId`; a `PackageName` alone is
   insufficient. This requirement does not serialize the graph-local ID as a
   stable cross-build identifier.
3. Imports, aliases, and public re-exports MUST preserve the target callable's
   canonical summary; they MUST NOT recompute, weaken, or strengthen it.
4. Source-defined callables compiled together MAY exchange summaries through
   compiler-internal state instead of a serialized artifact, but observed
   behavior MUST be identical to separate compilation with complete metadata.
5. Missing, malformed, or incompatible summary metadata yields `Unknown`.
   Effect checking then follows the conservative call-site rules in
   `effects.md`; a local alias or re-export MUST NOT repair unavailable
   metadata.
6. This metadata authorizes no effect by itself. Bang/non-bang context rules
   remain the only v0.1 source-level authorization boundary.
7. Compiler-provided standard-library metadata MUST resolve from the trusted
   catalog in `../stdlib/effect-summaries.yaml`. An implementation MAY compile
   that catalog into another representation, but the resulting callable
   identities, own summaries, callback positions, and re-export targets MUST be
   identical.

### Visibility

Top-level declarations, imports, and struct members are private to their defining module by default.

`pub` marks a declaration, import, or direct struct member as externally visible. Struct-member forms are `pub field`, `pub field mut`, `pub prop`, `pub prop mut`, `pub fn`, and `pub overload fn`.

Visibility is owner-constrained: a struct member is externally accessible only when both its owning struct and the member are public. Marking a member `pub` inside a private struct does not expose either declaration.

Code in the defining module MAY access private members. From another module:

1. stored-field reads and assignments require an externally accessible field,
2. property reads and direct calls to required property accessors require an externally accessible property,
3. receiver-method calls and concrete-owner callable references require an externally accessible inherent method family,
4. struct-literal construction follows the all-stored-fields rule in `bindings-structs.md`.

Accessing a non-public item or member from outside its defining module MUST fail with `VISIBILITY_VIOLATION`.

#### Externally Reachable Type Contracts

A type position is an externally reachable type contract when source outside the
position's defining module can use the declaration that owns it without first
violating visibility.

Externally reachable type contracts are:

1. parameter types, return types, and generic bounds of a `pub` free function,
2. generic bounds declared by a public `struct`, `enum`, `trait`, or type alias,
3. the declared type of a `pub const` and the target of a `pub type` alias,
4. every payload type of a public enum variant,
5. the declared type of an externally accessible stored field or property,
6. parameter types, return types, and generic bounds of an externally accessible inherent method,
7. parameter types, return types, and generic bounds of every method contract in a public trait,
8. the generic bounds, complete implemented-trait reference, and target type of
   an externally reachable conformance family, and
9. every associated-type binding in that family.

An externally reachable conformance family is formed by a public trait head and
a public target type head. Language-intrinsic and externally accessible catalog
type heads count as public.

The following are not externally reachable type contracts:

1. a private top-level declaration,
2. a private member of a public struct,
3. any member of a private struct, even when that member is written with `pub`,
4. a public nominal declaration's private stored representation, and
5. the header and associated bindings of a conformance whose trait or target
   type head is private.

Every externally reachable type contract MUST be externally nameable after
transparent alias expansion (`VISIBILITY_VIOLATION` otherwise). Validation uses
this closed procedure:

1. Resolve names, imports, aliases, generic arity, and projection syntax in the
   declaration's lexical scope.
2. Reject alias cycles and apply every ordinary type-position restriction before
   visibility closure depends on the resulting type.
3. Expand transparent aliases and normalize every projection that is
   declaration-time normalizable.
4. Walk the finite resulting type structure in written left-to-right order,
   descending through generic arguments, tuples, callable parameters/results,
   references, ownership carriers, unions in their legal positions, and every
   other written type component.
5. For each nominal declaration or trait contract reached by that walk, apply
   the following closed nameability test:
   1. language-intrinsic and externally accessible catalog declarations are
      nameable,
   2. a declaration owned by the current file set or package is nameable only
      when it is public, and
   3. in package mode, a declaration canonically owned by another package is
      nameable only when the module that owns the current contract contains a
      `pub import` whose resolved target is that exact canonical declaration.
      The public import MAY use an `as` spelling and MAY itself resolve through
      a chain of public re-exports that preserve that declaration identity.
   Reject the first declaration that fails this test.
6. Stop at a nominal declaration boundary after checking its written type
   arguments. The walk MUST NOT inspect that nominal declaration's stored fields,
   variant payloads, or other representation members; those declarations are
   validated separately when they are externally reachable.

The package-mode bridge in Rule 5.3 is contract-module-local. A private import,
a `pub import` in a sibling module, a public transparent type alias, a `pub
import` whose target declaration is a transparent alias, or a direct dependency
independently added by a downstream consumer does not satisfy it. Re-export the
underlying nominal or trait declaration itself instead.
Only `pub import` bindings already resolved by ordinary import resolution enter
the module's bridge index. Visibility closure performs no separate re-export
lookup or cycle traversal.
This keeps validation local to the declaration and its module import table. A
public re-export establishes an outward spelling only; it does not change the
target declaration's canonical type or trait identity. Same-package declarations
remain nameable through their canonical module paths and do not require a local
re-export.

A transparent type alias or import alias introduces only a spelling. Its own
visibility does not participate in the walk. A private alias MAY therefore occur
in a public contract when its complete expansion contains only externally
nameable nominal types and trait contracts. Conversely, a public alias does not
make a private nominal target public and does not replace the contract-local
`pub import` required for a declaration owned by another package. Public alias
declarations remain importable only through their public alias name, while
semantic type identity remains the expanded target under `type-system.md`.

A generic alias application satisfies the alias declaration's bounds before the
alias is expanded. A bound discharged by a concrete argument and unique impl does
not survive as an external contract requirement. A symbolic obligation must
already occur in the owning declaration's generic well-formedness environment;
when that owner is public, the bound is itself an externally reachable contract
and its trait must pass the same nameability test. A private alias therefore
cannot conceal a caller obligation on a private or unbridged foreign trait.

Generic parameters are valid symbolic contract components. `Self` is externally
nameable through its public owner, and an associated projection is externally
nameable when its declaring bound is public and the projection satisfies
`traits.md`. A downstream caller MAY instantiate a public generic declaration
with a type private to that caller's own module; that substitution does not
retroactively invalidate the generic declaration. A type private to a different
module cannot make a concrete target nameable there.

Visibility closure is declaration-site validation. It MUST NOT be deferred until
an import, call, projection, or inferred local later uses the declaration. A
public declaration with an unnameable contract type is invalid even when no
external source uses it. Local inference inside a type's defining module remains
unrestricted by this rule, and a public nominal value MAY contain private
representation selected under the exclusions above.

A compilation artifact or module interface that exposes a type contract MUST
carry enough canonical alias-expanded type information to reconstruct that
contract without resolving a private source alias or private import spelling. It
MUST also carry each exposing module's public re-export binding name and
canonical target so a downstream package can resolve a bridged foreign
declaration through the direct dependency's interface. It MAY retain a public
alias spelling as presentation metadata. Same-build and separate-compilation
behavior MUST be identical.

Validation precedence is:

1. name/import visibility, alias arity and cycles, projection legality, and
   ordinary type-position validation precede this closure check,
2. declaration-specific structural checks that determine the contract retain
   their existing precedence, including forbidden `pub extern`, inheritance,
   overload-family, and property-accessor checks,
3. within a trait impl, the more specific order in `traits.md` applies: impl
   authorization and coherence precede header visibility closure; header closure
   precedes associated-binding completeness; and each complete binding's
   ownership and reference checks precede that binding's visibility closure, and
4. after the applicable checks establish a valid contract, visibility closure precedes
   function or accessor body checking, const initializer checking, and every
   downstream use-site check.

Extern visibility boundary:

1. `extern` declarations are module-internal in v0.1.
2. `pub extern ...` is forbidden and MUST fail with `EXTERN_VISIBILITY_FORBIDDEN` (see `unsafe-ffi.md`).

Nominal-sum visibility and imports:

1. `pub enum E { ... }` exports enum type `E` and its variant constructors through enum namespace (`E::V`).
2. Private `enum E { ... }` keeps both enum type and enum-qualified constructor paths module-private.
3. Per-variant visibility modifiers are not supported in baseline syntax and MUST fail with `ENUM_VARIANT_VISIBILITY_FORBIDDEN`.
4. A catalog `sum type` has the visibility of its catalog owner declaration.
5. Source-enum and catalog-sum variants are not standalone import symbols in
   v0.1:
   1. `import m::E` then `E::V` is valid, subject to visibility,
   2. `import m::E as F` then `F::V` is valid and preserves the canonical owner
      and variant identities,
   3. direct `import m::V` for a nominal-sum variant is invalid and MUST fail
      with `IMPORT_NOT_FOUND`.

### Name Resolution Order

For unqualified value/type names, lookup order MUST be deterministic:

1. Active local value-binding scopes, searched from innermost to outermost and
   stopping at the first matching parameter, pattern binding, or `let`/`let
   mut` binding.
2. Current module's top-level declarations.
3. Explicit imports in current module.
4. Prelude bindings.

Local binding activation, nested shadowing, and same-scope redeclaration are
defined in `bindings-structs.md`. A same-scope local collision fails with
`LOCAL_BINDING_REDECLARATION` before lookup and therefore never forms an
ambiguous local candidate set.

If multiple candidates exist at another resolution tier, compilation MUST fail with `NAME_AMBIGUOUS`, except explicit overload sets as defined below.

Bindings found in higher tiers shadow lower tiers. Within the local tier, a
nearer active lexical scope shadows an outer scope. Local declarations are not
hoisted and cannot affect lookup at an earlier source position.

After context-specific classification, if no candidate exists in any applicable
tier for an ordinary unqualified identifier lookup, compilation MUST fail with
`NAME_NOT_FOUND`. A form with a more specific diagnostic keeps that diagnostic;
for example, an unresolved assignment target uses
`ASSIGN_TARGET_NOT_ASSIGNABLE`, and an unresolved interpolated-string
placeholder uses `FSTRING_UNRESOLVED_NAME`. `IMPORT_NOT_FOUND` remains specific
to a failed `import` path or imported symbol.

Binding-introducing pattern identifiers are not ordinary unqualified name lookups.
Pattern-specific binding/constructor disambiguation is defined in `control-flow.md`.

### Qualified Variant Resolution

In expression and pattern position, a `::` path MAY name a source-enum or
catalog-sum variant:

1. Resolution selects the longest leading module prefix, then resolves the next
   segment in the type namespace as the nominal sum owner. Exactly one variant
   segment MUST remain.
2. The final segment MUST name a variant declared by that owner. Imports and
   import aliases of the owner preserve this lookup.
3. Expression position yields the unit, positional, or named-payload
   constructor declared by the variant. Pattern position yields the
   corresponding variant pattern.
4. A qualified braced form is classified from the resolved declaration kind:
   a nominal sum owner plus variant is a named-payload variant form, while a
   struct type path is a struct form.
5. Dot syntax remains ordinary value-member lookup. It MUST NOT consult the type
   namespace for variants; if the receiver spelling has only a type binding,
   `Owner.Variant` fails with `NAME_NOT_FOUND`.
6. These expression/pattern rules do not affect type-position `::`
   classification or associated-type projection.

### Syntax-owned Tensor Classification

The bounded tensor language forms in `implementation-boundary.md` are
classified before ordinary unqualified lookup:

1. A complete `TensorSchemaExpr` is selected by grammar and does not resolve its
   leading contextual spelling through the value namespace.
2. A same-spelled declaration such as `fn view(...)` remains legal and shadows
   lower-tier value bindings for ordinary uses such as `view(...)`, but it
   cannot capture `view{...}` when the complete token sequence matches the
   schema-literal grammar.
3. `AxisNameLit` and `AxisIndexLit` construct the canonical
   `std::tensor::AxisName` and `std::tensor::AxisIndex` identities directly.
   Same-spelled type or value declarations do not affect their identity.
4. Dedicated tensor dtype brackets and `ReduceByCandidateSuffix` are not
   spelling-owned literals; they retain the canonical declaration/receiver
   identity gates defined in `syntax.md` and `../stdlib/tensor.md`.
5. Outside these closed forms, all tensor-related spellings use the ordinary
   lookup tiers and shadowing rules in this chapter.

Type-position `::` resolution distinguishes module/type paths from associated-type projections after contextual type spellings and the longest module prefix are resolved:

1. The exact spelling `Self` in type position is contextual and is classified before ordinary lookup. In an allowed owner context, the next segment begins an associated-type projection; outside one, `Self` or `Self::Name` fails with `SELF_TYPE_INVALID` and is never retried as a module, import alias, generic parameter, or concrete type.
2. If the first non-module segment resolves to an in-scope generic parameter, a following segment begins an associated-type projection and the complete path MUST follow `traits.md`.
3. Projection interpretation in Rules 1-2 takes precedence over module-path fallback. A valid projection has exactly one associated-name segment after its base; an unknown, ambiguous, or chained projection uses `ASSOCIATED_TYPE_PROJECTION_INVALID` rather than fallback.
4. Otherwise, resolve the longest leading module prefix and first remaining segment under ordinary name and visibility rules before classifying any suffix. An unresolved or inaccessible base therefore uses `IMPORT_NOT_FOUND` or `VISIBILITY_VIOLATION`, not an associated-projection diagnostic.
5. If the segment from Rule 4 resolves to a concrete type declaration or a type import alias and another segment remains, the suffix is a concrete-base projection attempt and MUST fail with `ASSOCIATED_TYPE_PROJECTION_INVALID`. This includes `dep::api::Count::Item` and `LocalCount::Item`.
6. A path whose longest module prefix is followed by exactly one type name, such as `pkg::module::Type`, is an ordinary module/type path and is not a projection.
7. Transparent type aliases preserve projection-origin provenance under `traits.md`; expansion MUST NOT turn an alias-hidden projection into an ordinary impl-head or overlap type.
8. Generic parameters shadow same-spelled outer module/type names within their
   declaration scope, except that no reserved built-in or contextual
   type-namespace name from `type-system.md` may be introduced as a generic
   parameter. They do not shadow generic parameters from the same or a
   containing declaration; `type-system.md` requires
   `GENERIC_PARAMETER_REDECLARATION` for those cases.

Method call resolution order for `recv.m(...)`:

1. Collect inherent methods on static type of `recv` whose callable name matches.
2. If inherent candidates exist:
   1. apply the owner-and-family visibility gate before applicability,
   2. fail with `VISIBILITY_VIOLATION` when that family is inaccessible at the call site, without falling back to trait candidates,
   3. resolve only within that inherent candidate set,
   4. select unique applicable candidate if one exists,
   5. fail with `OVERLOAD_AMBIGUOUS` if multiple applicable candidates remain,
   6. fail with `METHOD_NOT_FOUND` if no inherent candidate is applicable.
3. If no inherent candidates exist, collect trait methods whose trait identifiers are name-resolvable at the call site under normal scope/import rules.
4. Resolve only within the trait candidate set.

Ambiguous trait method candidates MUST fail with `TRAIT_METHOD_AMBIGUOUS`.

Fully qualified trait calls (for example `TraitName.m(recv, ...)`) bypass this method lookup ambiguity.

Method candidate visibility constraints:

1. Trait candidate collection is call-site scoped.
2. Existence of an `impl Trait for Type` alone MUST NOT make trait methods callable when `Trait` is not name-resolvable at the call site.
3. Trait methods from non-resolvable trait identifiers MUST NOT enter the candidate set.
4. If no inherent candidate exists and no trait candidates remain after visibility filtering, resolution MUST fail with `METHOD_NOT_FOUND`.
5. A trait re-export is name-resolvable at the tier where the re-export is
   visible and contributes the target trait's methods once. The re-export path
   preserves the target trait and callable identities and MUST NOT create a
   duplicate candidate.
6. Selecting `TypeName.method` as a call target or named callable reference
   considers only inherent or associated callables declared on `TypeName`.
   Trait methods implemented by that type do not become concrete-owner members.
   If no such concrete-owner member exists, resolution MUST fail with
   `METHOD_NOT_FOUND`; use receiver method syntax or `TraitName.method`.
7. A concrete-owner callable reference from outside the defining module requires both an externally accessible owner and an externally accessible inherent method family. An inaccessible matching family fails with `VISIBILITY_VIOLATION` rather than `METHOD_NOT_FOUND`.

### Explicit Overload Sets

v0.1 allows overloading only when explicitly marked.

Rules:

1. If two or more declarations in the same owner scope share the same callable name, each declaration in that set MUST be declared with the `overload` keyword.
2. Owner scope is:
   1. module scope for free functions,
   2. struct body for inherent methods,
   3. trait body for trait method declarations,
   4. impl body for trait implementation methods.
3. `foo` and `foo!` are distinct callable names and form separate overload sets.
4. Two overload declarations that differ only by return type are forbidden and MUST fail with `OVERLOAD_RETURN_ONLY_DIFFERENCE`.
5. Overload declarations MUST be pairwise disjoint by applicability in v0.1. If a single argument-type vector can match more than one declaration, compilation MUST fail with `OVERLOAD_OVERLAP`.
6. If same-name declarations exist in one owner scope and at least one declaration omits `overload`, compilation MUST fail with `OVERLOAD_MARKER_REQUIRED`.
7. Every declaration in a module-level or inherent overload set MUST have the same declared visibility. After Rule 1 succeeds, a mixed public/private set MUST fail with `OVERLOAD_VISIBILITY_MISMATCH`.
8. Trait requirements inherit their trait's visibility, and trait-impl methods have no independent visibility, so trait and impl overload sets are uniform by construction.
9. Declaration-set validation applies `OVERLOAD_MARKER_REQUIRED` before `OVERLOAD_VISIBILITY_MISMATCH`, then checks return-type-only differences and applicability overlap.

Signature equivalence and overlap model (v0.1):

1. Overload signature key is `(callable_name, bang_marker, receiver_type, parameter_type_list)`.
2. Return type is not part of overload signature key and MUST NOT participate in candidate ranking.
3. Generic parameter names are alpha-normalized for signature comparison:
   1. for example `f[T](x: T)` and `f[U](x: U)` are equivalent for overlap checking.
4. Receiver type is part of signature key; `Self`, `&Self`, and `&mut Self` are distinct declared signatures.
5. Overlap checking is semantic, not textual:
   1. if any argument-type vector can satisfy two declarations under v0.1 applicability rules, declarations overlap.
6. Exact-duplicate signature keys and semantic-overlap pairs are both rejected through `OVERLOAD_OVERLAP` in v0.1.
7. Generic bound lists do not participate in overload signature keys and MUST NOT be used to prove two declarations disjoint. Declarations whose parameter structures overlap remain `OVERLOAD_OVERLAP` even when their bounds differ.
8. Projection-origin types do not provide a negative fact for overlap checking, whether still symbolic or concretely normalized and whether written directly or reached through an alias. A projected parameter therefore MUST NOT be proved disjoint from an ordinary parameter type by consulting an associated binding.

Overload resolution:

1. Candidate collection is limited to declarations selected by normal lookup tiering.
2. Before applicability, a bare fixed-width integer literal argument is typed
   without using any unresolved candidate's parameter type as expected-type
   context. It follows the defaulting rules in `type-system.md`;
   candidate-local representability MUST NOT create or rank candidates. This
   rule does not remove contextual typing that another expression family
   defines independently.
3. Applicability uses exact parameter-type matching after receiver auto-borrow/reborrow adjustments only.
4. Numeric promotion, deref chaining, and user-defined implicit conversion are not part of v0.1 overload applicability.
5. If no free-function or owner-qualified candidate is applicable, compilation
   MUST fail with `CALL_ARG_TYPE_MISMATCH`. Receiver-method lookup retains its
   `METHOD_NOT_FOUND` no-applicable-candidate rule above.
6. If exactly one applicable candidate remains, that declaration is selected.
7. If more than one applicable candidate remains, compilation MUST fail with `OVERLOAD_AMBIGUOUS`.
8. After structural selection and generic inference choose one candidate, its declared trait bounds are checked. A failed bound uses `TRAIT_BOUND_NOT_SATISFIED`; bounds do not cause fallback to another structurally overlapping declaration because the generic-bound overlap rule above rejects such overload sets.
9. Associated-result normalization occurs only after a candidate and its complete generic argument vector are known. Projection normalization MUST NOT rank candidates or reverse-infer an underconstrained generic parameter.
10. Direct bottom compatibility from `type-system.md` is not an overload
    applicability conversion. While a set is unresolved, an exact `Never`
    argument matches only an exact `Never` parameter. If no candidate remains,
    Rule 5 applies even though the argument expression has no normal
    continuation.

These rules govern call expressions. Named callable-value coercion uses expected
`Fn` compatibility from `calls-references.md`; its assignability-based
selection can retain multiple members of a declaration set that is disjoint
under exact call applicability.

Method lookup implicit-conversion policy (v0.1):

1. The receiver is the only position that MAY use implicit borrow adjustment for method lookup.
2. Non-receiver arguments MUST NOT gain candidates via numeric promotion,
   candidate-local integer-literal typing, or other implicit conversion during
   overload applicability checks.
3. This policy applies equally to inherent and trait method candidate sets.

### Type Alias Expansion and Cycle Handling

Import graph cycles are allowed in v0.1.

Because top-level executable statements are not part of v0.1, import cycles MUST NOT introduce runtime initialization behavior differences.

Type-alias expansion is transparent substitution as defined in `type-system.md`.

Resolution and cycle rules:

1. Alias right-hand-side type paths resolve in the alias declaration's lexical module scope.
2. Public aliases re-export only the alias name; after visibility and lookup succeed, type identity is still the expanded target type.
3. Generic alias parameters shadow outer type names only inside that alias declaration's target type expression.
4. Type-alias expansion MUST be acyclic; cyclic alias expansion MUST fail with `TYPE_ALIAS_CYCLE`.

Direct and mutual recursion between function declarations is allowed.

Constant dependency expansion MUST be acyclic; cyclic constant dependencies MUST fail with `CONST_CYCLE`.
