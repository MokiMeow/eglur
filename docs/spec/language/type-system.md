# Type System Baseline

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Type Checking Baseline

v0.1 type checking prioritizes local predictability over global inference.

Rules:

1. Function parameter types are required in declarations.
2. Named `fn`/`fn!` and `extern "..." fn` declarations MUST provide explicit return types.
3. Missing explicit return type on a named function, method, or extern declaration MUST fail with `FN_RETURN_TYPE_REQUIRED`.
4. Local `let`/`let mut` bindings MAY infer type from initializer expressions.
5. Local `let`/`let mut` bindings MUST include an initializer expression in v0.1.
6. Local `let`/`let mut` bindings whose initializer remains underdetermined after local inference constraints (for example `let x = None`) MUST fail with `BINDING_TYPE_UNDETERMINED`.
7. If both annotation and initializer are present, including in a `const`
   declaration, the initializer type MUST be assignable to the annotation type
   (`ASSIGNMENT_TYPE_MISMATCH` otherwise).
8. `const` initializers MUST be const-evaluable under `validation-matrix.md` (`CONST_INIT_NOT_CONST` otherwise).
9. For expression-body functions (`fn ... = expr`), type of `expr` MUST be assignable to function return type.
10. For block-body functions (`fn ... { ... }`), function value comes from the
    explicit block-result and normal-continuation semantics in `control-flow.md`:
    completed `yield` edges, reachable `()` fallthrough, or `Never` when no
    normal result edge exists.
11. Block-body functions MUST NOT use implicit tail-expression return; trailing expression statements do not satisfy non-`Unit` return requirements.
12. If block-body computed result type is not assignable to function return type, compilation MUST fail with `RETURN_TYPE_MISMATCH`.

Local inference MUST close while checking each binding initializer. No later
use of a local binding may add constraints to that initializer.
The binding group introduced by the declaration is absent while its initializer
is resolved and checked, then becomes active atomically under
`bindings-structs.md`; type inference MUST NOT provisionally expose that group.
A compiler-known schema adapter MAY define a closed expression-local typing
order in `../stdlib/catalog-signatures.md`: when adapter construction and
invocation form one complete expression, the invocation input may determine an
expected type for an inline callback before that callback body is checked.
Canonical `then` or `apply` forms count only after their specified desugaring
produces that same complete invocation expression. This rule never permits a
provisional local binding or constraints that cross an initializer boundary.

Uninitialized local declarations:

1. `let x: T` and `let mut x: T` without initializer are not baseline v0.1 declarations.
2. They MUST fail with `BINDING_TYPE_UNDETERMINED`.
3. v0.1 does not define default local initialization, zero initialization, or late definite-assignment for local declarations.
4. Assignment after declaration is reassignment, not first initialization; immutable `let` reassignment remains invalid under `IMMUTABLE_BINDING_ASSIGN`.

Local binding and parameter patterns:

1. Function, method, trait, and extern parameters MUST use identifier binding patterns only.
2. Parameter wildcard `_`, tuple, struct, variant, literal, alias, or-pattern, and rest patterns MUST fail with `PARAM_PATTERN_UNSUPPORTED`.
3. This restriction keeps named-argument binding, receiver validation, and parameter documentation tied to stable parameter names.
4. Local `let`/`let mut` patterns MAY use:
   1. identifier binding patterns,
   2. wildcard `_`, which evaluates the initializer but introduces no binding,
   3. Unit pattern `()`, which introduces no binding,
   4. tuple patterns recursively composed only of accepted local binding patterns.
5. Struct, variant, literal, alias, or-pattern, and rest `..` patterns in local binding position MUST fail with `LET_PATTERN_UNSUPPORTED`.
6. Unit local binding pattern `()` is irrefutable only when the initializer type
   is `Unit`; tuple local binding patterns are irrefutable only when the
   initializer type is a tuple with exactly the same arity at every nested tuple
   position (`LET_PATTERN_TYPE_MISMATCH` otherwise). Unary tuple binding patterns
   are spelled `(p,)`.
7. Each identifier introduced by an accepted tuple local binding pattern receives the corresponding projected tuple element type.
8. If a local binding type annotation is present, it constrains the whole initializer value before destructuring; v0.1 does not support per-element annotations inside patterns.
   Initializer typing and annotation compatibility precede destructuring shape
   checks; an incompatible initializer uses `ASSIGNMENT_TYPE_MISMATCH`, while
   a compatible initializer with an invalid Unit/tuple pattern shape uses
   `LET_PATTERN_TYPE_MISMATCH`.
9. `let mut` mutability applies to every identifier introduced by the accepted local binding pattern.
10. A single local binding pattern MUST NOT introduce the same identifier more than once (`LET_PATTERN_DUPLICATE_BINDING` otherwise).
11. An accepted local binding pattern MUST NOT collide with an earlier binding
    in the same lexical scope (`LOCAL_BINDING_REDECLARATION` otherwise).
12. Refutable decomposition remains explicit through `match`; v0.1 does not define refutable local binding patterns.

Return statement typing:

1. `return` statements MUST type-check against enclosing function/method return type.
2. Bare `return` is valid only for `Unit` return type (`RETURN_VALUE_REQUIRED` otherwise).
3. `return <expr>` is invalid for `Unit` return type (`RETURN_VALUE_FORBIDDEN` otherwise).
4. For non-`Unit` return type, `return <expr>` expression type MUST be assignable to enclosing return type (`RETURN_TYPE_MISMATCH` otherwise).
5. `return` outside function/method bodies, including inside closure bodies, MUST fail with `RETURN_OUTSIDE_FUNCTION`.


Call argument typing:

1. Positional arguments bind parameters by declared parameter order.
2. Named arguments bind parameters by declared parameter name; calls through `Fn` values use the positional-only rule in `calls-references.md`.
3. Positional-after-named usage is forbidden and MUST be rejected during static checking (`CALL_POSITIONAL_AFTER_NAMED`).
4. Duplicate named bindings to one parameter are forbidden and MUST be rejected during static checking (`CALL_DUPLICATE_NAMED_ARG`).
5. Unknown named argument identifiers are forbidden and MUST be rejected during static checking (`CALL_UNKNOWN_NAMED_ARG`).
6. After binding, each required parameter MUST have exactly one argument:
   1. missing required arguments fail with `CALL_MISSING_ARG`,
   2. positional arguments that do not bind any parameter fail with `CALL_TOO_MANY_ARGS`.
7. After binding, each argument expression type MUST be assignable to the bound parameter type (`CALL_ARG_TYPE_MISMATCH` otherwise).
8. When the bound parameter type is `Fn(...) -> ...`, argument assignability uses callable type compatibility from `calls-references.md`.
   Arity, parameter-type, or result-type incompatibility fails with `CALL_ARG_TYPE_MISMATCH` unless a more specific callable diagnostic applies. An overload set that cannot be selected as a function value fails with `CALLABLE_VALUE_NO_MATCH` before argument binding.


### Unit Value and Pattern

`Unit` is an intrinsic singleton type. Its sole value has the canonical source
spelling `()`.

Rules:

1. `UnitExpr` (`()`) has type `Unit` without expected-type inference.
2. The prelude exposes `Unit` as a type only. It does not expose a built-in value
   named `Unit`; absent an explicit user value binding with that spelling, bare
   `Unit` in expression position fails ordinary value lookup with
   `NAME_NOT_FOUND`.
3. `UnitPattern` (`()`) matches only `Unit`, binds no names, is irrefutable for
   `Unit`, and covers the complete `Unit` value domain in `match` exhaustiveness
   analysis. Bare `Unit` in pattern position remains an identifier binding under
   `control-flow.md`; it is not a Unit pattern.
4. A local `let () = expr` is accepted exactly when `expr` has type `Unit`
   (`LET_PATTERN_TYPE_MISMATCH` otherwise). Parameter restrictions remain
   unchanged: `()` is not an accepted parameter pattern in v0.1.
5. `()` is not a tuple expression, tuple pattern, or type spelling. Tuple arity
   remains one or greater, and the sole type spelling is `Unit`.
6. `()` is const-evaluable.
7. Canonical source rendering of the Unit value and Unit pattern is `()`. This
   rule does not add a required `Display[Unit]` implementation; structured
   conformance observation continues to use `kind: Unit`.


### `Never` and Direct Bottom Compatibility

`Never` is the intrinsic type of computations that have no normal
continuation. It has no values.

Rules:

1. The sole source spelling is `Never`. It is a canonical intrinsic type, not a
   primitive scalar, nominal declaration, user-definable empty enum, or alias
   for `!`.
2. `Never` is non-generic. `Never[T]` and every other application with type
   arguments MUST fail with `TYPE_ARG_ARITY_MISMATCH`.
3. The type-namespace name `Never` is reserved under `syntax.md` and
   `algebraic-data.md`. The prelude re-exports the same intrinsic identity; it
   does not declare a second type.
4. `Never` has no literal, constructor, or built-in value binding. Bare
   `Never` in expression position follows ordinary value lookup and, absent an
   explicit value binding with that spelling, fails with `NAME_NOT_FOUND`.
5. `Never` is ownership-free and reference-free. It MAY appear in ordinary
   non-extern value type positions, including parameters, returns, locals,
   fields, enum payloads, aliases, generic arguments, associated bindings, and
   callable components. The closed extern and raw-pointer subset remains
   governed by `unsafe-ffi.md`.
6. After a well-formed expected value type `T` has been established
   independently, an expression whose exact alias-expanded type is `Never` is
   assignable to `T`. No type other than alias-expanded `Never` is assignable to
   expected `Never` through this rule.
7. Rule 6 is a directional compatibility rule, not a general subtype lattice,
   implicit conversion, runtime coercion, or value construction. It does not
   lift through a type constructor: for example, `Seq[Never]` is not assignable
   to `Seq[T]`, and `Fn() -> T` is not assignable to `Fn() -> Never` unless
   `T` is `Never`.
8. Bottom compatibility does not establish an expected type, select an
   operator family, or widen unresolved overload applicability. It is applied
   only after the surrounding syntax and type position are valid and a target
   type or callable has been selected by the ordinary rules.
9. Omitted generic arguments continue to use exact local evidence:
   1. if an independently established expected result or another earlier exact
      constraint fixes a parameter `T`, a `Never` argument is checked by Rule 6
      and does not replace that choice,
   2. if `T` is otherwise unresolved and an ordinary first-order parameter
      position matches an exact `Never` argument, that evidence MUST infer
      `T = Never`,
   3. conflicting exact constraints, unresolved parameters, and projection-only
      constraints retain `TYPE_ARG_INFERENCE_FAILED`; no fallback type is
      invented.
10. Exact overload call applicability remains the rule in
    `modules-resolution.md`. While an overload set is unresolved, a `Never`
    argument exactly matches only a `Never` parameter. Expected-`Fn` callable
    selection instead uses the ordinary assignability relation after its target
    shape is known.
11. `Never` receives no automatic trait implementations. Ordinary explicit impl
    declarations remain subject to the orphan and coherence rules in
    `traits.md`.
12. Exact-`Never` typing introduces a separate **no-normal-continuation** control
    fact. Strict evaluation contexts propagate that fact when evaluation cannot
    reach their result construction or invocation. The enclosing expression
    retains its ordinarily determined type unless its own result rule is defined
    from normal-completion edges; such a rule yields `Never` when no result edge
    exists. Expected-type compatibility MUST NOT erase the control fact.
13. The compiler MUST still resolve and validate every syntactically present
    expression, declaration, branch, and callable body under the ordinary type,
    effect, ownership, and diagnostic-precedence rules. A preceding
    no-normal-continuation expression does not suppress source validation or
    effect-summary collection.
14. Existing form restrictions precede Rule 6. In particular,
    `return <expr>` remains forbidden for a `Unit` function, and a `result {}`
    block without its required explicit final `yield` remains invalid even when
    some contained expression has type `Never`.
15. A structurally uninhabited type is not canonical `Never` and does not by
    itself carry the no-normal-continuation fact. Structural inhabitance is the
    separate bounded analysis below.
16. A call expression introduces the no-normal-continuation fact exactly when
    the selected callable's normalized result type is `Never`. A callable
    declared with another result type does not acquire that fact from body
    analysis, recursion, or an implementation-specific divergence proof.


### Bounded Inhabitance Facts

The compiler computes a conservative inhabitance fact only for exhaustiveness,
reachability, and empty-match validation. This fact is distinct from type
identity, assignability, traits, layout, and control-flow divergence.

Rules:

1. Only an otherwise well-formed, alias-expanded type is classified. Earlier
   name, arity, type-position, bound, alias-cycle, and recursive-layout failures
   retain their ordinary diagnostics.
2. The fact lattice is `Uninhabited < Unknown < Inhabited`:
   1. `Inhabited` proves that at least one value form exists,
   2. `Uninhabited` proves that no value can exist,
   3. `Unknown` proves neither and MUST be treated as possibly inhabited by
      exhaustiveness checking.
3. Product and sum nodes use these monotone operations:
   1. a product is `Uninhabited` if any component is `Uninhabited`, is
      `Inhabited` if every component is `Inhabited`, and is `Unknown`
      otherwise; the empty product is `Inhabited`,
   2. a sum is `Inhabited` if any alternative is `Inhabited`, is
      `Uninhabited` if every alternative is `Uninhabited`, and is `Unknown`
      otherwise; the empty sum is `Uninhabited`.
4. The following closed dispositions apply after ordinary well-formedness:

   | Type form | Inhabitance rule |
   |---|---|
   | `Never` | `Uninhabited` |
   | `Unit`, `Bool`, `Tri`, concrete numeric/domain scalar types, and `Str` | `Inhabited` |
   | tuple and source `struct` storage | product of element or stored-field facts |
   | source `enum` and catalog `sum type` | sum of variant payload products; a unit variant is an inhabited empty product |
   | `Opt[T]` | `Inhabited` because `None` exists |
   | `Res[T, E]` | sum of `T` and `E` |
   | normalized error `Union[...]` | sum of its member facts |
   | `Box[T]`, `Own[T]`, `&T`, and `&mut T` | the fact for `T` |
   | well-formed `Seq`, `Vec`, `Map`, `Set`, `PVec`, `PMap`, and `PSet` applications | `Inhabited` because an empty value exists |
   | `Fn(...) -> R` and `Task[T]` | `Inhabited`; a callable or task handle may exist even when its computation never completes |
   | a generic parameter, unresolved associated projection, or opaque/catalog family without a rule above | `Unknown` |

5. Raw pointers do not enter this analysis in v0.1: their source use is confined
   to the non-pattern-matchable extern relay boundary in `unsafe-ffi.md`.
6. A nominal variant is impossible exactly when its payload product is
   `Uninhabited`. Impossible variants do not contribute required match coverage;
   an explicit arm that can match only an impossible variant is unreachable
   under `control-flow.md`.
7. Consequently, `Opt[Never]` has only the possible `None` variant,
   `Res[T, Never]` has no possible `Err` variant, and
   `Res[Never, Never]` is uninhabited. These facts do not rewrite any of those
   types to another type.
8. Transparent aliases are expanded before graph construction. A source alias
   never changes the fact of its target.
9. Recursive classification uses a deterministic least fixed point over the
   finite reachable declaration/type graph, initialized to `Uninhabited` and
   updated by Rule 3. A recursive edge that re-enters the same nominal head with
   a different type-argument vector is cut to `Unknown`; implementations MUST NOT
   enumerate a transformed generic instance orbit.
10. Rule 9 classifies a recursion with no base value, such as
    `enum Loop { Next(Box[Loop]) }`, as `Uninhabited`; a unit/base variant makes
    the sum `Inhabited`; and a field behind an always-inhabited empty container,
    such as `struct S { field xs: Seq[S] }`, does not make the product
    uninhabited.
11. Only exact `Uninhabited` authorizes a zero-arm match. `Unknown` is rejected
    conservatively. No inhabitance fact supplies a trait proof, a layout or niche
    fact, an extern ABI, an implicit conversion, or the exact-`Never`
    no-normal-continuation fact.


### Array Literal Typing

Array expression surface `[...]` is fixed to sequence semantics in v0.1.

Rules:

1. `ArrayExpr` (`[e1, ..., en]`) has baseline type `Seq[T]`.
2. Element expressions MUST type-check to one common element type `T`.
3. Method resolution on `ArrayExpr` follows `Seq[T]` method surface.
4. `Vec[T]`-only APIs require explicit conversion via `to_vec()` before use.
5. Empty array literal `[]` requires expected type context; if unresolved, compilation MUST fail with `BINDING_TYPE_UNDETERMINED`.
6. A dedicated `Vec` literal surface is not part of v0.1.
7. This keeps literal normalization fixed to sequence semantics before mutable/random-access capability is requested explicitly.


### Map Literal Typing

Map expression surface `[k1: v1, ..., kn: vn]` is fixed to `Map` semantics in
v0.1. The empty spelling is `[:]`.

Rules:

1. A non-empty `MapExpr` has type `Map[K,V]`.
2. Key expressions MUST type-check to one common key type `K`, and value
   expressions MUST independently type-check to one common value type `V`.
3. Expected `Map[K,V]` context may flow into the literal and its entries during
   ordinary expression-local checking. No later use of a local binding may add
   constraints to an already checked map initializer.
4. Every inferred or expected key type MUST satisfy the `Eq + Hash` bounds on
   `Map`; failure uses `TRAIT_BOUND_NOT_SATISFIED`.
5. Empty map literal `[:]` requires expected `Map[K,V]` context. If either type
   parameter remains unresolved, compilation MUST fail with
   `BINDING_TYPE_UNDETERMINED`.
6. Bare identifiers in key position are ordinary value references. In
   particular, a `Map[Str,V]` literal writes string keys as string literals;
   `[name: value]` never means `["name": value]` implicitly.
7. Method resolution on `MapExpr` follows the required `Map[K,V]` surface from
   `../stdlib/map.md`.
8. Map entry types remain subject to the ownership-free and reference-free
   v0.1 `Map` domain. Literal syntax does not widen that domain.
9. `MapExpr` is not const-evaluable in v0.1. Its use as a `const` initializer
   fails with `CONST_INIT_NOT_CONST`.

Construction order, duplicate-key behavior, and insertion-order semantics are
defined in `../stdlib/map.md`.


### Tuple Value Typing and Projection

Tuple expressions provide fixed-arity heterogeneous product values.
Tuple arity is one or greater in v0.1.
Unary tuple values and types use a trailing comma (`(expr,)`, `(Type,)`) so they remain distinct from parenthesized grouping forms.

Rules:

1. A tuple expression `(e0, e1, ..., en)` has tuple type `(T0, T1, ..., Tn)` where each `Ti` is the type of `ei`; for arity 1 this is spelled `(e0,)` and `(T0,)`.
2. Tuple element projection uses zero-based numeric postfix syntax:
   1. `t.0` projects the first element,
   2. `t.1` projects the second element,
   3. tuple-index tokens are interpreted as base-10 indices using the same underscore rules as ordinary decimal integer literals,
   4. a tuple index is a static field designator, not a value expression.
3. If the receiver expression of tuple projection does not have tuple type, compilation MUST fail with `TUPLE_PROJECTION_NON_TUPLE`.
4. If the projection index is greater than or equal to the tuple arity, compilation MUST fail with `TUPLE_PROJECTION_OUT_OF_BOUNDS`.
5. The result type of `t.N` is the `N`th element type of the receiver tuple.
6. Tuple projection is static field projection:
   1. general expression indexing, including `t[i]`, is not part of v0.1,
   2. dynamic tuple element selection is not part of v0.1,
   3. tuple projection does not imply `Seq`/`Vec` methods on tuple values.
7. Tuple values MAY participate in pattern matching and irrefutable binding where pattern semantics permit it; those binding rules are defined separately from projection.


### Generic Type Parameters

v0.1 supports first-order type parameters on declarations:

1. `fn`, `struct`, `enum`, `trait`, `impl`, and `type` declarations MAY use `GenericParams`.
2. Higher-kinded type parameters are out of scope.
3. Const generics and specialization are out of scope.
4. A generic parameter MAY declare one or more nominal trait bounds inline, for example `[T: Clone]` or `[T: Readable + Debug]`.
5. A generic parameter name MUST be unique within its `GenericParams` and MUST NOT redeclare a generic parameter from a containing declaration scope (`GENERIC_PARAMETER_REDECLARATION` otherwise). This does not prohibit shadowing a same-spelled module or concrete type name under `modules-resolution.md`.
6. Each bound path MUST resolve to a visible trait at the declaration site, and one normalized trait reference MUST NOT appear more than once for the same parameter (`TRAIT_BOUND_INVALID` otherwise).
7. `where` clauses, negative bounds, associated-type equality constraints, and higher-ranked bounds are not part of v0.1.
8. Both unconstrained and bounded generic parameters range only over ownership-free, reference-free types in v0.1. `calls-references.md` defines reference-free validation.
9. Generic ownership code MUST expose the wrapper in its declaration, for example `fn pass[T](x: Own[T]) -> Own[T]`; inferring `T = Own[U]` for a plain `T` position is forbidden.
10. Generic borrowing code MUST expose the reference outside the parameter, for example `fn inspect[T](x: &T) -> i64`; inferring `T = &U` or `T = &mut U` for a plain `T` position is forbidden.

Bound checking:

1. `A + B` is conjunction: every listed bound is required, and bound order has no type-semantic meaning.
2. Bound declarations use two passes. The first resolves every bound path and validates trait kind, generic arity, and duplicates without yet discharging bounds declared by a referenced generic trait. The complete normalized bound set collected by that pass forms the current declaration's **generic well-formedness environment**.
3. The environment in Rule 2 is available during the second-pass validation of
   dependent bound arguments and while checking that declaration's remaining
   header, signature, associated bindings where applicable, and body. It may
   discharge an exact symbolic obligation such as `T: Mark` while validating
   `U: Uses[T]`, `Guarded[T]`, or another dependent generic application. While
   checking a trait default body, `traits.md` augments this environment with
   exactly the complete normalized symbolic current-trait obligation for `Self`
   and its finite compiler-known prerequisites; no other implicit declaration
   assumption is added.
4. A symbolic discharge in Rule 3 is not a concrete impl lookup, does not infer an unknown type argument, and does not escape the declaration. Once every argument of an obligation is concrete, a unique applicable impl is still required.
5. Mutually declared symbolic assumptions MAY coexist, but only exact normalized assumptions or ordinary nominal impl lookup discharge obligations. Re-entering any other unresolved obligation cycle is not proof and fails under the cycle-safe solver rule in `traits.md`.
6. Bound validation begins only after the complete generic argument vector is known. It applies to:
   1. inferred and explicit callable instantiations,
   2. expected-`Fn` instantiations of named generic functions,
   3. generic type applications in annotations, fields, enum payloads, aliases, and nested type expressions,
   4. generic trait references in bounds and impl headers,
   5. generic impl candidates after impl-header inference.
7. For every application in Rule 6, the complete argument vector is substituted through each declared bound before obligation resolution. Each symbolic obligation MUST be present in the current generic well-formedness environment or follow from a unique applicable impl; each concrete obligation MUST have a unique applicable impl (`TRAIT_BOUND_NOT_SATISFIED` otherwise).
8. Validation for one type application follows this precedence:
   1. generic arity, type-name resolution, alias validity, and projection validity,
   2. ownership-position validation,
   3. reference-free abstraction-slot validation,
   4. trait-bound satisfaction.
   A well-formed application that reaches Rule 8.4 and lacks a required assumption or impl uses `TRAIT_BOUND_NOT_SATISFIED`.
9. When more than one bound is unsatisfied for one parameter, the leftmost source bound is the primary failure; additional failures MAY be related diagnostics.
10. Generic impl bounds are obligations of that impl: an impl is applicable only when its inferred type arguments satisfy every declared bound.
11. Bound satisfaction is nominal trait-impl lookup. Shape, axis, dtype, device, value predicates, and catalog-only `requires` predicates MUST NOT participate in source trait-bound solving.
12. Bounds do not authorize ownership-bearing or reference-bearing substitutions and MUST NOT invoke ownership-aware or reference-aware trait solving.
13. Every associated type is ownership-free and reference-free in v0.1; projection cannot conceal an ownership root, conditional ownership carrier, or reference node.
14. After aliases and associated projections normalize, the resulting type MUST be revalidated in its enclosing source type position before layout analysis or expression typing. Normalization never makes a type legal in a position where its normalized form is forbidden.

Type argument inference:

1. Implementations MUST infer omitted callable type arguments using call argument types and expected result type context.
2. If omitted type arguments remain underconstrained or ambiguous, compilation MUST fail with `TYPE_ARG_INFERENCE_FAILED`.
3. v0.1 explicit call-site type arguments use `callee::[T0, ...](...)` surface.
4. Bracket-only call-site explicit type arguments (for example `f[i64](x)`) are not part of v0.1 core grammar. The bracket-call surface is reserved for the dedicated stdlib constructor forms named in Rule 9.
   Generic-call attempts using this surface MUST fail with `TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN`.
5. Call-site explicit type arguments MUST be all-or-none:
   1. a call either supplies all generic type arguments explicitly with `::[...]`, or supplies none and relies on inference,
   2. supplying only a prefix/subset of a generic callee's type arguments MUST fail with `TYPE_ARG_INFERENCE_FAILED`.
6. Closure parameter annotations participate in ordinary local type checking.
7. General expression ascription (`expr: T`) is not part of v0.1 core grammar.
8. Local binding annotations (`let x: T = ...`) remain the baseline explicit-type escape hatch outside call-site explicit type arguments and closure parameter annotations.
9. The dedicated `std::tensor` constructor dtype-bracket surface (`zeros[T](...)`, `ones[T](...)`, `nan[f32](...)`, `rand![f32](...)`) is not generic call-site type-argument syntax and is specified by `stdlib/tensor.md`. Its dtype argument is normalized through transparent type aliases before the resolved constructor's accepted dtype family is checked; a normalized type outside that family fails with `CALL_ARG_TYPE_MISMATCH`.
10. Inference or explicit type arguments that place an ownership-root type in an ordinary generic parameter MUST fail with `OWNERSHIP_TYPE_POSITION_FORBIDDEN`.
11. Inference or explicit type arguments that are not reference-free under `calls-references.md` MUST fail with `REFERENCE_TYPE_POSITION_FORBIDDEN`.
12. Rule 10 precedes Rule 11 when the same argument violates both constraints. Rule 11 precedes diagnostics that would arise only after substituting the forbidden reference into a signature, including `NON_BANG_MUT_PARAM_FORBIDDEN` and `REF_ESCAPE_FORBIDDEN`.
13. Bound satisfaction is checked only after ordinary local inference identifies a complete type-argument vector; bounds MUST NOT trigger global inference or select an otherwise underconstrained type argument.
14. Associated-type projections in a callee result type normalize after their projection base is inferred from a non-projected argument position or otherwise fixed by ordinary expected-type inference. A result projection alone MUST NOT be inverted to discover its base type; if the base remains underconstrained, Rule 2 applies.
15. The same non-inversion rule applies to projected parameter and expected-`Fn` positions. Unifying a concrete type with `T::Item` does not infer `T`; a type parameter that appears only as a projection base remains underconstrained and MUST fail with `TYPE_ARG_INFERENCE_FAILED` unless an explicit immediate-call type argument or another non-projected position fixes it.
16. A type parameter in a complete matching position MUST bind to an entire
    normalized error union when that union is the independently typed actual
    value. For example, matching `Res[T, E]` against
    `Res[i64, Union[IoError, ParseError]]` binds `E` to the complete
    `Union[IoError, ParseError]` type.
17. A structural `Union[...]` node contributes no member-wise inference
    constraints. Implementations MUST NOT pair members by source order or
    canonical identity, subtract fixed members, partition the actual union, or
    search member permutations to infer a parameter nested inside the union.
18. A parameter nested only inside structural union nodes MUST be fixed by
    complete explicit immediate-call type arguments or by independent
    non-union inference positions. Once the complete substitution is known,
    the specialized union participates in ordinary assignability checking. If
    no such evidence completes the substitution, Rule 2 applies.
19. Trait impl headers have no explicit impl-argument application surface.
    `traits.md` therefore rejects an impl-introduced parameter strictly below a
    normalized structural union node. A parameter in the complete error
    position remains governed by Rule 16.

Generic nominal constructor inference:

1. These rules apply to generic `StructLiteralExpr` values and qualified generic
   source-enum or catalog-sum `VariantConstructorExpr` values. The reserved
   unqualified `Some`, `None`, `Ok`, and `Err` constructors retain the contextual
   diagnostic rules in `failure.md`.
2. v0.1 has no explicit constructor-head type-argument syntax. Generic owners use
   `Cell { ... }`, `Choice::Left(...)`, or `Choice::End`; a local binding
   annotation such as `let cell: Cell[i64] = Cell { value: 1 }` is the baseline
   explicit-type escape hatch.
3. A constructor expression may determine its complete owner type-argument
   vector from only:
   1. an independently established expected type whose alias-expanded,
      normalized outer nominal owner is the same declaration, and
   2. ordinary first-order matching of independently typed field or payload
      expression types against the corresponding declared field or payload
      types.
4. Expected type context in Rule 3.1 MUST be established without using the
   constructor expression or a later use of its completed local binding. It may
   come from an annotated binding, a declared return or `yield` target, or a
   statically selected call parameter whose type contains no unresolved callable
   type arguments when the constructor is checked.
5. When Rule 3.1 fixes the complete vector, the compiler MUST specialize all
   field or payload types before checking their expressions. Rule 3.2 does not
   collect competing inference constraints in this case. An expression not
   assignable to its specialized slot MUST fail with `CALL_ARG_TYPE_MISMATCH`,
   not `TYPE_ARG_INFERENCE_FAILED`.
6. Without a complete expected owner type, an initializer expression may
   contribute a Rule 3.2 constraint only when it can be typed without expected
   type information derived from a constructor slot that still contains an
   unresolved owner parameter. Once other valid constraints determine the
   complete vector, remaining context-dependent initializer expressions are
   checked against their specialized slots but do not select that vector. An
   incompatible remaining expression MUST fail with
   `CALL_ARG_TYPE_MISMATCH`.
7. First-order matching in Rule 3.2 expands transparent aliases and descends
   through equal nominal type constructors, tuples, and callable structure. It
   MUST NOT invert an associated-type projection, enumerate trait-bound
   witnesses, inspect unselected variants, or use assignability alternatives to
   choose an owner argument.
8. Expected-owner and independently typed non-literal constraints are applied
   before a fixed-width integer literal is considered. If they complete the
   vector, the specialized slot contextually types the literal. Otherwise the
   still-unconstrained literal defaults under the rules below before its type
   contributes a constructor constraint.
9. Constraints admitted under Rules 3-8 MUST yield exactly one complete,
   consistent owner type-argument vector. An underconstrained vector, multiple
   valid vectors, or inconsistent collected constraints MUST fail with
   `TYPE_ARG_INFERENCE_FAILED`. This expression-specific failure precedes
   `BINDING_TYPE_UNDETERMINED` when the constructor is the initializer of an
   unannotated local binding.
10. A parameter absent from the selected variant payload or every stored field
    remains unresolved without Rule 3.1. In particular, unit variants,
    zero-field generic structs, and phantom generic parameters require an
    independently established expected owner type.
11. After a complete vector is known, generic arity, ownership-position,
    reference-free, and trait-bound validation follows the ordering already
    defined in this section. Bounds validate the result; they never complete or
    disambiguate constructor inference.
12. Type aliases do not introduce constructors. An expected alias may normalize
    to the resolved constructor owner's instantiation, but the constructor path
    itself MUST still name the nominal declaration. A uniquely resolved alias
    used as a struct or variant constructor head MUST fail with
    `CONSTRUCTOR_TARGET_KIND_MISMATCH`; unresolved heads retain
    `NAME_NOT_FOUND`.
13. Generic arguments are not written in variant patterns. The already known
    scrutinee type supplies the owner instantiation for pattern validation.
14. Nominal owner and variant resolution plus target-kind and field/payload
    shape validation precede constructor type-argument inference. A wrong
    resolved target kind uses `CONSTRUCTOR_TARGET_KIND_MISMATCH` before payload
    validation. A payload-form mismatch uses
    `CONSTRUCTOR_PAYLOAD_FORM_MISMATCH` before arity or field validation.
    For a matching form, unknown, duplicate, missing, or arity-invalid
    initializer entries retain their ordinary constructor diagnostics even when
    an owner parameter would otherwise remain unresolved.


### Fixed-Width Integer Literal Typing

An integer literal retains its exact mathematical magnitude until ordinary
expression-local inference gives it a concrete type.

Rules:

1. A fixed-width integer expected type contextually types an integer literal
   only when that expected type is established independently of the literal and
   independently of unresolved overload candidates.
2. Such established contexts include:
   1. an explicit binding, parameter-default, field, return, or container-element
      type,
   2. the concrete opposite operand of an otherwise type-valid scalar expression,
   3. a parameter of a uniquely resolved non-overloaded callable,
   4. a callable parameter or nominal-constructor field/payload slot whose
      generic type argument is already fixed by explicit call-site type
      arguments, expected result type, or independently typed non-literal
      argument/initializer expressions.
3. For unary `-` applied directly to an integer literal under an expected signed
   fixed-width type, representability is checked against the negated
   mathematical value. This permits the minimum value of that signed type, such
   as `let min: i8 = -128`.
4. After explicit type arguments, expected-result constraints, and non-literal
   argument or initializer constraints have been applied to a uniquely resolved
   generic callable or nominal constructor, every remaining unconstrained
   integer literal defaults to `i64` before its type contributes to generic
   argument inference.
5. A generic bound, including the closed `FixedWidthInteger[T]` predicate, MUST
   NOT enumerate fitting integer dtypes or select a narrower or wider type for
   an otherwise unconstrained literal.
6. Parameter types from an unresolved overload set MUST NOT contextually type an
   integer literal, make a candidate applicable merely because the literal value
   is representable, or rank one candidate over another. An integer literal
   still unconstrained at overload applicability defaults to `i64`.
7. Once a concrete fixed-width type is established, the literal's mathematical
   value MUST be representable in that type. A statically known failure,
   including failure after the default in Rule 4, MUST use `INT_OVERFLOW`.
8. Later uses of a completed initializer MUST NOT add literal-type constraints
   retroactively.


### Ownership Type Positions

Ownership-bearing type placement is intentionally closed in v0.1:

1. `Own[T]` is valid only when `T` is ownership-free and reference-free.
2. The only ownership-bearing aggregate types are `Opt[Own[T]]` and `Res[Own[T], E]`, with ownership-free, reference-free `T` and `E`.
3. Direct and conditional ownership-root types may be complete parameter, return, or local-binding types.
4. Borrow views `&R` and `&mut R` are valid when `R` is an ownership-root type.
5. Any other alias-expanded type containing `Own[...]`, including an alias target that would hide the root shape, MUST fail with `OWNERSHIP_TYPE_POSITION_FORBIDDEN`.
6. The complete transfer, carrier, and discharge rules are defined in `ownership-boundary.md`.
7. Opaque standard-library resource payload types remain ownership-free as payloads, but source acquisition exposes them only through the valid root shapes in Rules 1-4.
8. Standard-library methods may be declared directly on a specific `Own[R]` resource instantiation without making `Own` a forwarding wrapper.
9. `Task[T]` is not a resource root: when `T` is ownership-free and reference-free it is an ownership-free immutable completion value under `concurrency.md`.


### Type Alias Expansion

Source `type` declarations define transparent type aliases, not nominal wrapper types.

Rules:

1. `type Name = Target` introduces `Name` as an alternate spelling for `Target`.
2. `type Name[T0, ..., Tn] = Target` introduces a generic transparent alias; each use of `Name[A0, ..., An]` substitutes the supplied type arguments for the alias parameters in `Target`.
3. Alias expansion is semantic-only and MUST NOT create a distinct runtime representation, constructor, destructor, trait implementation scope, method namespace, ownership/effect boundary, or visibility identity.
   A fully applied alias may still spell its expanded struct's existing method owner in `Alias.method`; the selection rule is in `modules-resolution.md`.
4. Type identity, assignability, overload applicability, trait matching, equality typing, public-API type closure, and diagnostics that depend on concrete type structure MUST operate on the alias-expanded type.
5. Generic alias uses MUST provide exactly the alias generic arity:
   1. missing or extra type arguments fail with `TYPE_ARG_ARITY_MISMATCH`,
   2. v0.1 does not support partial type-alias application,
   3. v0.1 does not infer omitted type arguments for type aliases from surrounding type context.
6. Non-generic alias uses MUST NOT provide type arguments (`TYPE_ARG_ARITY_MISMATCH` otherwise).
7. Alias expansion MUST be acyclic; cyclic alias expansion fails with `TYPE_ALIAS_CYCLE`.
8. The expanded target remains subject to all ordinary type-position validation rules:
   1. raw pointer restrictions still use `RAW_POINTER_TYPE_UNSUPPORTED`,
   2. unsupported `dyn` positions still use `DYN_TRAIT_UNSUPPORTED`,
   3. the reserved `Union[...]` structural type remains accepted only as a complete `Res[T, E]` error-channel argument and otherwise fails with `UNION_TYPE_UNSUPPORTED`.
9. A source type alias MUST NOT be used to introduce a general value-level union, sum type, or variant constructor surface in v0.1.
10. Standard-library declarations use the closed [catalog signature metalanguage](../stdlib/catalog-signatures.md); for example, `union type AxisSel = AxisIndex | AxisName` is a catalog declaration and is not source-level `TypeDecl` syntax.
11. Human-facing diagnostic prose MAY preserve an alias spelling when that
    spelling is accessible at the diagnostic site. Type-valued structured
    diagnostic fields MUST use the alias-expanded canonical spelling in
    `../errors/rendering.md`; alias spelling never changes conformance-level
    semantic behavior.
12. Public-API visibility follows the externally reachable type-contract closure in `modules-resolution.md`. After alias-application well-formedness and bound satisfaction succeed, the check ignores source alias visibility, rejects nominal or trait declarations that are not externally nameable after expansion, and does not turn a public alias or a public import targeting an alias into a visibility bridge for either a private nominal target or a foreign package declaration lacking the required contract-module-local nominal/trait `pub import`.
13. A generic alias bound discharged by a concrete argument does not survive transparent expansion. A symbolic alias obligation must be available in the current declaration's generic well-formedness environment under the ordinary bound-checking rules above; an alias cannot introduce an implicit caller obligation.
14. A compilation artifact or module interface MUST NOT require a downstream module to resolve a private alias in order to reconstruct an exposed type contract.

### Raw Pointer Type Positions and Confined Relay

Raw pointer type grammar exists to support FFI signatures. v0.1 additionally
permits a compiler-inferred, lexically confined relay value; it does not provide
general low-level pointer programming.

Rules:

1. Source `*const T` and `*mut T` type expressions MAY appear in extern declaration parameter and return types only when the pointee type satisfies `unsafe-ffi.md` extern ABI subset rules.
2. A source raw-pointer type expression in any non-extern type position MUST fail with `RAW_POINTER_TYPE_UNSUPPORTED`.
3. Non-extern positions include ordinary function/method parameters and returns, local annotations, struct fields, properties, trait method signatures, type aliases, generic arguments, and nested occurrences inside those positions.
4. An unannotated immutable local MAY receive an inferred raw-pointer type only as a relay value under the closed origin, region, flow, and sink rules in `unsafe-ffi.md`.
5. Rule 4 is not a general inferred-type exception: a relay value MUST NOT enter an aggregate, generic instantiation, callable signature/value, mutable local, return, capture, or any other non-relay position.
6. A forbidden use of an otherwise valid inferred relay value MUST fail with `RAW_POINTER_USE_UNSUPPORTED`; it does not retroactively make the local initializer a `RAW_POINTER_TYPE_UNSUPPORTED` source position.
7. Unsupported raw pointer forms inside extern declarations continue to use `EXTERN_SIGNATURE_UNSUPPORTED`, not `RAW_POINTER_TYPE_UNSUPPORTED`.
8. Raw-pointer argument compatibility for a valid extern sink is exact except for the one-way `*mut T` to `*const T` weakening defined in `unsafe-ffi.md`; other mismatches use `CALL_ARG_TYPE_MISMATCH`. This weakening does not participate in branch-result common-type selection.

### Dynamic Trait Object Type Positions

`dyn Trait` grammar is reserved for future explicit dynamic dispatch, not a v0.1 baseline runtime object model.

Rules:

1. In non-extern type positions, explicit `dyn Trait` MUST fail with `DYN_TRAIT_UNSUPPORTED`.
2. Non-extern positions include ordinary function/method parameters and returns, local annotations, struct fields, properties, trait method signatures, type aliases, generic arguments, and nested occurrences inside those positions.
3. `dyn Trait` inside extern declaration parameter or return types remains an unsupported ABI signature form and MUST fail with `EXTERN_SIGNATURE_UNSUPPORTED`, not `DYN_TRAIT_UNSUPPORTED`.
4. Trait object usage without explicit `dyn` continues to fail with `DYN_MUST_BE_EXPLICIT`.

### Union Type Positions and Error-Union Assignability

The reserved structural type head `Union[A, B, ...]` makes fallible pipeline error surfaces explicit without introducing general value-level union programming in v0.1. It is not a nominal generic type and cannot be shadowed or redefined.

Rules:

1. A source `Union` head MUST supply at least two type arguments. Bare `Union` and `Union[A]` fail with `UNION_TYPE_ARITY_MISMATCH`; `Union[]` is already rejected by the non-empty `TypeArgs` grammar. This arity check precedes union-position and extern-signature validation.
2. In v0.1 baseline, a well-formed `Union[...]` is semantically accepted only when it denotes the complete error type argument `E` of `Res[T, E]`.
3. Accepted error-union positions include function/method return types, local annotations, parameter types, field/property/trait signatures, and generic arguments only when the enclosing type position is `Res[T, E]` and the union expression is `E` after parentheses and union flattening.
4. A well-formed `Union[...]` in a non-`Res` error-channel position MUST fail with `UNION_TYPE_UNSUPPORTED`.
5. A well-formed `Union[...]` inside an extern declaration parameter or return type remains an unsupported ABI signature form and MUST fail with `EXTERN_SIGNATURE_UNSUPPORTED`, not `UNION_TYPE_UNSUPPORTED`.
6. A `Union[...]` nested inside another type constructor that itself serves as `E` (for example `Res[T, Seq[Union[E1, E2]]]`) is not a v0.1 error union and MUST fail with `UNION_TYPE_UNSUPPORTED`.
7. General union construction, projection, narrowing, and pattern matching are not part of v0.1 outside `Res` error-channel propagation.
8. A normalized error union MUST:
   1. flatten nested unions,
   2. remove duplicate semantically identical member types,
   3. remove `Never` members as impossible alternatives,
   4. compare member sets without regard to source order.
9. Human-facing diagnostic prose SHOULD preserve first-occurrence source order
   after flattening, even though type equality ignores order. Type-valued
   structured diagnostic fields instead use the sorted canonical spelling in
   `../errors/rendering.md`.
10. A non-union error type `S` is assignable to target error union `U` iff `S` is assignable to at least one normalized member of `U`.
11. Source error union `Union[S1, ..., Sn]` is assignable to target error union `Union[T1, ..., Tm]` iff every normalized source member is assignable to at least one normalized target member.
12. A multi-member source error union is not assignable to a non-union target error type without explicit handling or mapping.
13. A normalized singleton union is semantically equivalent to its sole member. For example, source `Union[A, A]` is well-formed and normalizes to `A`.
14. A source union whose normalized member set becomes empty after Rule 8,
    including `Union[Never, Never]`, is semantically equivalent to `Never`.
15. When checking assignment, call arguments, returns, or `yield` values of type `Res[Tsrc, Esrc]` against expected type `Res[Tdst, Edst]`, implementations MUST compare the error arguments using these error-union assignability rules; this rule does not make arbitrary generic type constructors covariant.

### Extern Declaration Typing

Extern declaration typing is constrained by `unsafe-ffi.md`.

Rules:

1. `pub extern ...` is forbidden in v0.1 (`EXTERN_VISIBILITY_FORBIDDEN`).
2. Extern ABI selector MUST belong to the v0.1 ABI subset from `unsafe-ffi.md`.
3. Extern ABI selectors outside the subset MUST fail with `EXTERN_ABI_UNSUPPORTED`.
4. Extern parameter/return types MUST belong to the v0.1 extern signature subset from `unsafe-ffi.md`.
5. Extern signatures using unsupported types MUST fail with `EXTERN_SIGNATURE_UNSUPPORTED`.
6. Extern declarations MUST include an explicit `ReturnType`; omitting it MUST fail with `FN_RETURN_TYPE_REQUIRED`.
7. v0.1 does not infer omitted extern return types as `Unit`; write `-> Unit` explicitly for no-result ABI declarations.

#### Type-check Diagnostics

Type checking failures in this section MUST use stable compile-time diagnostic codes defined in `../ERRORS.md`:

`FN_RETURN_TYPE_REQUIRED`, `BINDING_TYPE_UNDETERMINED`, `LOCAL_BINDING_REDECLARATION`, `GENERIC_PARAMETER_REDECLARATION`, `PARAM_PATTERN_UNSUPPORTED`, `LET_PATTERN_UNSUPPORTED`, `LET_PATTERN_TYPE_MISMATCH`, `LET_PATTERN_DUPLICATE_BINDING`, `CONST_INIT_NOT_CONST`, `CONST_FORWARD_REFERENCE`, `CONST_CYCLE`, `IMMUTABLE_BINDING_ASSIGN`, `MUTABLE_CAPTURE_FORBIDDEN`, `ASSIGN_TARGET_NOT_ASSIGNABLE`, `ASSIGNMENT_TYPE_MISMATCH`, `VAR_DECL_FORBIDDEN`, `STRUCT_FIELD_DEFAULT_FORBIDDEN`, `STRUCT_INIT_UNKNOWN_FIELD`, `STRUCT_INIT_DUPLICATE_FIELD`, `STRUCT_INIT_MISSING_FIELD`, `STRUCT_INIT_PROPERTY_FORBIDDEN`, `PROP_ACCESSOR_MISSING`, `PROP_ACCESSOR_SIGNATURE_MISMATCH`, `PROPERTY_ASSIGN_FORBIDDEN`, `TYPE_ALIAS_CYCLE`, `TYPE_ARG_INFERENCE_FAILED`, `TYPE_ARG_ARITY_MISMATCH`, `TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN`, `TRAIT_BOUND_INVALID`, `TRAIT_BOUND_NOT_SATISFIED`, `ASSOCIATED_TYPE_PROJECTION_INVALID`, `ABSTRACT_NUMERIC_KIND_VALUE_POSITION`, `NUMERIC_OPERAND_NOT_NUMERIC`, `NUMERIC_CHECKED_CONTEXT_REQUIRED`, `INT_OVERFLOW`, `LOGICAL_OPERAND_TYPE_MISMATCH`, `COMPARISON_TYPE_MISMATCH`, `DOMAIN_INVALID_VALUE`, `OVERLOAD_MARKER_REQUIRED`, `OVERLOAD_VISIBILITY_MISMATCH`, `OVERLOAD_RETURN_ONLY_DIFFERENCE`, `OVERLOAD_OVERLAP`, `OVERLOAD_AMBIGUOUS`, `NON_BANG_MUT_PARAM_FORBIDDEN`, `REF_ESCAPE_FORBIDDEN`, `REFERENCE_TARGET_INVALID`, `REFERENCE_TYPE_POSITION_FORBIDDEN`, `METHOD_RECEIVER_INVALID`, `SELF_TYPE_INVALID`, `RANGE_EXPR_UNSUPPORTED`, `RETURN_OUTSIDE_FUNCTION`, `RETURN_VALUE_REQUIRED`, `RETURN_VALUE_FORBIDDEN`, `RETURN_TYPE_MISMATCH`, `EXTERN_VISIBILITY_FORBIDDEN`, `EXTERN_SIGNATURE_UNSUPPORTED`, `EXTERN_ABI_UNSUPPORTED`, `RAW_POINTER_TYPE_UNSUPPORTED`, `RAW_POINTER_USE_UNSUPPORTED`, `DYN_TRAIT_UNSUPPORTED`, `UNION_TYPE_ARITY_MISMATCH`, `UNION_TYPE_UNSUPPORTED`, `OWNERSHIP_PROFILE_DISABLED`, `OWNERSHIP_PATH_UNBALANCED`, `OWNERSHIP_TYPE_POSITION_FORBIDDEN`, `OWNERSHIP_TRANSFER_REQUIRES_MOVE`, `OWNERSHIP_DISCARD_FORBIDDEN`, `MOVE_REQUIRES_OWN`, `OWN_REQUIRES_NON_OWN`, `OWN_REQUIRES_LOCAL_BINDING`, `DROP_OWN_REQUIRES_MOVE`, `RESOURCE_DROP_FORBIDDEN`, `OWNERSHIP_DOUBLE_MOVE`, `OWNERSHIP_USE_AFTER_DISCHARGE`, `OWNERSHIP_GUARD_CONSUME_FORBIDDEN`, `CONSTRUCTOR_TARGET_KIND_MISMATCH`, `CONSTRUCTOR_PAYLOAD_FORM_MISMATCH`, `CALL_POSITIONAL_AFTER_NAMED`, `CALL_DUPLICATE_NAMED_ARG`, `CALL_UNKNOWN_NAMED_ARG`, `CALL_MISSING_ARG`, `CALL_TOO_MANY_ARGS`, `CALL_ARG_TYPE_MISMATCH`, `CALLABLE_VALUE_NO_MATCH`, `TUPLE_PROJECTION_NON_TUPLE`, `TUPLE_PROJECTION_OUT_OF_BOUNDS`, `FSTRING_UNRESOLVED_NAME`, `FSTRING_INVALID_PLACEHOLDER`, `RESERVED_BUILTIN_NAME`, `TYPE_RECURSIVE_LAYOUT_REQUIRES_INDIRECTION`.

### Algebraic Data Typing Hooks

Source-enum and catalog-sum typing rules are defined in `algebraic-data.md` and
are part of v0.1 program-model type checking.

Type checker requirements:

1. Qualified variant constructor expressions MUST type-check to the constructor
   signatures defined by the resolved source enum or catalog sum:
   1. unit constructors (`Owner::V`),
   2. positional constructor calls (`Owner::V(...)`),
   3. named constructor initializers (`Owner::V { ... }`).
2. A uniquely resolved constructor head MUST match the written constructor
   surface's required semantic kind before payload validation
   (`CONSTRUCTOR_TARGET_KIND_MISMATCH` otherwise).
3. The written constructor form MUST match the resolved declaration before
   arity, fields, payload expressions, or owner inference are checked
   (`CONSTRUCTOR_PAYLOAD_FORM_MISMATCH` otherwise).
4. Variant patterns MUST check their unit, positional, or named payload shape and
   binding types against the same resolved variant declaration.
5. Any inline recursive nominal layout cycle without recognized indirection MUST fail with `TYPE_RECURSIVE_LAYOUT_REQUIRES_INDIRECTION`.
6. User bindings that shadow reserved built-in or contextual type-namespace names (`Opt`, `Res`, `Own`, `Box`, `Fn`, `Union`, `Never`, `Self`) or redefine reserved value constructors (`Some`, `None`, `Ok`, `Err`) MUST fail with `RESERVED_BUILTIN_NAME` under `algebraic-data.md` and `syntax.md`.

### Numeric Promotion Contract

For binary numeric arithmetic operators (`+`, `-`, `*`, `/`, `/.`, `%`):

1. Scalar expression typing and tensor primitive typing MUST use the same promotion lattice from `../stdlib/conventions.md`.
2. When operand dtypes are known statically, result dtype selection MUST be deterministic from that lattice.
3. Scalar fixed-width integer fault eligibility and `checked {}` requirements are governed by `../stdlib/conventions.md` and the rules below.
4. `Tri` is excluded from this numeric lattice and MUST NOT be accepted as a numeric arithmetic operand in v0.1.
5. If either operand is non-numeric (including `Tri`), compilation MUST fail with `NUMERIC_OPERAND_NOT_NUMERIC`.
6. `/.` uses fractional-division rules from `../stdlib/conventions.md` and MUST NOT use integer-division runtime fault semantics.
7. `+` is not string concatenation in v0.1; string composition is provided by `std::prelude::concat` and formatting APIs.

For unary numeric negation (`-`), the operand MUST have a numeric scalar dtype.
A non-numeric operand, including `Tri`, MUST fail with
`NUMERIC_OPERAND_NOT_NUMERIC`. Successful result and failure behavior follows
the scalar fixed-width integer and IEEE-754 rules below without binary
promotion.

For scalar fixed-width integer arithmetic:

1. Potentially faulting forms are unary `-` and binary `+`, `-`, `*`, `/`, and `%` after operand typing and numeric promotion, including an implicit integer-to-integer promotion conversion that can fail with `INT_OVERFLOW`.
2. A potentially faulting form MUST appear inside an active lexical `checked {}` block unless either:
   1. all operands are const-evaluable and evaluation succeeds; or
   2. every implicit promotion conversion either has a const-evaluable source
      operand and succeeds or is statically range-preserving over the complete
      source dtype range, and the promoted operation is one of:
      1. integer `%` with a const-evaluable nonzero divisor,
      2. signed integer `/` with a const-evaluable divisor other than `0` or
         `-1`,
      3. unsigned integer `/` with a const-evaluable nonzero divisor,
      4. binary `+` or `*` with two `Bool` operands, which therefore executes
         over the total `u8` result ranges `0..=2` or `0..=1` respectively.
3. An operation outside `checked` that is not one of those forms MUST fail with `NUMERIC_CHECKED_CONTEXT_REQUIRED`.
4. Const-evaluable failing operations retain their cause diagnostics (`INT_OVERFLOW`, `INT_DIV_BY_ZERO`, or `INT_REM_BY_ZERO`) and MUST NOT be replaced by `NUMERIC_CHECKED_CONTEXT_REQUIRED`.
5. Operand and promotion errors, including `NUMERIC_OPERAND_NOT_NUMERIC`, take precedence over the checked-context requirement.
6. `/.` is excluded because its result uses floating/complex IEEE-754 value semantics.
7. Explicit scalar `wrapping_add`, `wrapping_sub`, and `wrapping_mul` calls are ordinary total calls and do not require `checked`.
8. Inside `checked`, the successful type of each arithmetic subexpression remains its promoted scalar dtype; the enclosing `checked` expression performs the `Res` wrapping defined in `failure.md`.
9. The statically total forms in Rule 2 are the complete portable v0.1 exemption set. Implementations MUST NOT make source acceptance depend on optional range analysis or optimizer proofs beyond those forms.
10. After promotion, integer `/` and `%` MUST use the truncating quotient and
    dividend-signed remainder law in `../stdlib/conventions.md`. Scalar `/` and
    `%` MUST agree exactly with tensor `std::tensor::prim::div` and
    `std::tensor::prim::rem`, including zero-divisor and signed-minimum edge
    behavior.

### Numeric Kind vs Concrete Dtype

Numeric umbrellas `Int`, `UInt`, `Integer`, `Float`, and `Complex` are abstract kinds in v0.1.

Rules:

1. Abstract numeric kinds MUST NOT appear in value-level type positions (parameter types, return types, local/field annotations, generic concrete arguments).
   Such usage MUST fail with `ABSTRACT_NUMERIC_KIND_VALUE_POSITION`.
2. Concrete value typing MUST use fixed-width dtypes (`i8/i16/i32/i64`, `u8/u16/u32/u64`, `f16/bf16/f32/f64`, `complex64/complex128`).
3. Core examples and conformance cases that require a default integer concrete dtype use `i64` in v0.1.

### Domain Scalar Types

`Nat` and `PositiveNat` are opaque concrete scalar domain types for non-negative and strictly-positive integer-valued API boundaries.

Core domains:

1. `Nat` represents mathematical integers in the closed range `0..=i64::MAX`.
2. `PositiveNat` represents mathematical integers in the closed range `1..=i64::MAX`.
3. Implementations MUST NOT expose representation, layout, pointer-size coupling, or ABI identity for either type.
4. `PositiveNat` is not a subtype of `Nat`; explicit conversion through `PositiveNat.to_nat` is required where a `Nat` value is needed.

Typing rules:

1. `Nat` and `PositiveNat` MAY appear in ordinary value-level type positions.
2. They are concrete domain scalar types, not fixed-width numeric dtypes, not tensor element `DType`s, and not kind-level numeric umbrellas.
3. They MUST NOT participate in numeric promotion, scalar arithmetic operators, tensor numeric primitive typing, or the cast matrix from `../stdlib/cast-matrix.md`.
4. Integer literals MAY be contextually typed as `Nat` or `PositiveNat` when the expected type is exactly that domain type.
   Expected-type contexts include annotations, call arguments, default arguments, known container element types, and the opposite operand of same-domain equality or relational comparisons.
5. A contextually typed integer literal MUST be accepted only when its mathematical value is in the target domain range (`DOMAIN_INVALID_VALUE` otherwise).
6. Without expected type context, integer literals continue to use the existing default concrete integer behavior; `Nat` is never inferred as the default integer type.
7. Explicit runtime conversion uses the prelude functions from `../stdlib/prelude.md`.
8. Non-const-evaluable conversion failures MUST return `Err(DomainError)` and preserve `DOMAIN_INVALID_VALUE` as the diagnostic cause code.

Comparison and formatting:

1. Equality operators (`==`, `!=`) are defined for operands with identical static domain type and compare mathematical values.
2. `Nat` and `PositiveNat` implement `Ord`; relational operators on operands with identical static domain type dispatch that canonical implementation and compare mathematical values.
3. Mixed `Nat`/`PositiveNat` operators are not implicit; use `PositiveNat.to_nat` before comparing across domains.
4. Stable formatting is defined by `../stdlib/prelude.md`.

Public API integer-domain classification:

This table is normative for v0.1 public API surfaces that may otherwise look like sizes, counts, indices, dimensions, ordinals, or byte counts. Fixed-width numeric parsers, FFI signatures, tensor element dtypes, and examples that merely demonstrate ordinary integer values use fixed-width numeric types because they are numeric-value APIs, not domain boundaries.

| Surface | Classification | Rationale and failure channel |
| --- | --- | --- |
| `Vec`, `Seq`, `Map`, `Set`, `Str`, `PVec`, `PMap`, and `PSet` lengths, capacities, element indices, slices, counts, and enumeration positions | `Nat` | These are zero-valid collection sizes or positions. Invalid signed runtime values fail at `Nat.from_i64` or contextual literal checking with `DOMAIN_INVALID_VALUE`; bounds/range failures after a valid `Nat` crosses the API boundary use the container's value-channel error where one exists. |
| Tensor dimensions, shapes, ranks, axis positions, schema `.with_sizes(...)` bindings, map_axis batch lengths, and tensor API ordinal arguments such as `argnum` | `Nat` | Tensor extents and axis ordinals are zero-valid non-negative domains. Invalid signed runtime values fail at `Nat` conversion or contextual literal checking with `DOMAIN_INVALID_VALUE`; tensor consistency failures after valid domain values use tensor-specific diagnostics and value-channel errors. |
| Positive-count inputs such as `Own[ByteReader].read_chunk!` / `Own[TcpStream].read_chunk!` `max_bytes` and `par_*` `workers` | `PositiveNat` | Zero is also invalid. Invalid signed runtime values fail at `PositiveNat` conversion or contextual literal checking with `DOMAIN_INVALID_VALUE` before IO/concurrency side effects that depend on the count. |
| `Duration.nanos`, `Duration.millis`, `Duration.seconds`, `Duration.as_nanos`, and `Duration.as_millis` | `Nat` | `Duration` is a non-negative elapsed-time value. Constructor argument domain failures use `DOMAIN_INVALID_VALUE`; constructor unit-scaling overflow remains `Err(TimeError)`. |
| `Duration.mul`, `Duration.div`, `Duration.scale`, and `Duration.parse` operands/input text | signed numeric or string validation | These are arithmetic/parser validation APIs rather than size/count boundaries. Invalid factors, malformed strings, parse overflow, arithmetic overflow, and duration underflow remain `Err(TimeError)`. |
| Tensor indexing coordinates: `IndexItem::At(i64)`, positional `IndexItem::Slice(i64 | None, i64 | None, i64 = 1)` (`start`, `stop`, `step`), and `IndexItem::Take(Tensor[i64] | TensorView[i64] | Seq[i64] | Vec[i64])` | signed `i64` coordinate domain | Gather/scatter coordinates intentionally use the tensor indexing coordinate dtype, not `Nat`, so callers do not convert through domain scalars. Both concrete readable tensor carriers are accepted without materialization. Non-`i64` coordinate inputs fail with `CALL_ARG_TYPE_MISMATCH`; bounds/shape failures use `IndexError`, `ShapeError`, and tensor diagnostics. |
| `for` range iteration bounds (`start .. end`, `start ..= end`) | signed `i64` arithmetic iteration | Range bounds are arithmetic sequence endpoints, not sizes or counts. Non-`i64` bounds fail with `FOR_ITERABLE_NOT_SEQUENCE`; descending ranges execute zero iterations in v0.1. |
| Diagnostic byte offsets and source coordinates, plus diagnostic `data` numeric payloads such as `index` and `phase_index` | serialized `i64` payload fields | Diagnostics use a small JSON-like interchange model with `Str | i64 | Bool` data values. Producers MUST emit semantically valid non-negative or positive values as required by each field, but these payloads are not user-callable API boundaries and do not use `Nat`/`PositiveNat`. |
| Host-observed filesystem byte-count returns such as `PathStat.size_bytes` and `copy!` | `u64` | These are returned host measurements, not input boundaries. They may exceed the v0.1 `Nat` range and failures to observe/copy are already represented by `IoError`. |

### Logical Operator Typing

For scalar logical operators (`&&`, `||`):

1. Both operands MUST have type `Bool` or `Tri` (`LOGICAL_OPERAND_TYPE_MISMATCH` otherwise).
2. If both operands have type `Bool`, the result type is `Bool` and the operator uses ordinary two-valued Boolean semantics.
3. If either operand has type `Tri`, the result type is `Tri`.
4. In mixed `Bool`/`Tri` logical operator typing, each `Bool` operand is lifted with `Bool.to_tri()` before applying the Strong Kleene (K3) rules from `../stdlib/logic.md`.
5. Both operand expressions MUST be type-checked before evaluation semantics are applied; short-circuiting does not suppress type checking of the right operand.
6. Evaluation order is left-to-right.
7. Short-circuiting is permitted only when the left operand alone fixes the K3 result:
   1. `false && rhs` and `Tri::False && rhs` yield the operator's false value (`false` for Bool-only, `Tri::False` when result type is `Tri`) without evaluating `rhs`,
   2. `true || rhs` and `Tri::True || rhs` yield the operator's true value (`true` for Bool-only, `Tri::True` when result type is `Tri`) without evaluating `rhs`.

### Equality Comparison Typing

For equality operators (`==`, `!=`) in scalar expression typing:

1. Both operand expressions are evaluated exactly once in left-to-right order.
2. If both operands are numeric scalar types, equality typing MUST first apply numeric promotion from `../stdlib/conventions.md`; the promoted concrete type is the comparison type.
3. Otherwise, both operands MUST have identical static type; no implicit cast or non-numeric coercion is allowed.
4. The comparison type MUST implement the canonical `std::prelude::Eq` trait.
5. Equality inside generic code requires an explicit applicable bound. In particular, comparing two values of type `T` requires `T: Eq`; the operator MUST NOT infer or add that bound.
6. After operand evaluation and any numeric promotion, `lhs == rhs` dispatches to the unique applicable `std::prelude::Eq.eq(&lhs, &rhs)` implementation.
7. `lhs != rhs` is exactly the Boolean negation of the Rule 6 result. A type MUST NOT override `!=` independently.
8. An inherent method, a local trait named `Eq`, or any intrinsic not identified as `std::prelude::Eq` MUST NOT alter operator dispatch.
9. If operand compatibility or the required canonical `Eq` implementation is absent, compilation MUST fail with `COMPARISON_TYPE_MISMATCH`.
10. Equality results have type `Bool`.
11. For floating scalar values, runtime/evaluation semantics of equality and ordering operators are defined by unified floating total-order rules in `../stdlib/conventions.md`.

### Relational Comparison Typing

For relational operators (`<`, `<=`, `>`, `>=`) in scalar expression typing:

1. Both operand expressions are evaluated exactly once in left-to-right order.
2. If both operands are numeric scalar types, relational typing MUST first apply numeric promotion from `../stdlib/conventions.md`; the promoted concrete type is the comparison type.
3. Otherwise, both operands MUST have identical static type; no implicit cast or non-numeric coercion is allowed.
4. The comparison type MUST implement the canonical `std::prelude::Ord` trait.
5. Relational comparison inside generic code requires an explicit applicable bound. In particular, comparing two values of type `T` requires `T: Ord`; the operator MUST NOT infer or add that bound.
6. After operand evaluation and any numeric promotion, the operator dispatches exactly once to the unique applicable `std::prelude::Ord.cmp(&lhs, &rhs)` implementation:
   1. `<` is true exactly for `Ordering::Less`,
   2. `<=` is true exactly for `Ordering::Less` or `Ordering::Equal`,
   3. `>` is true exactly for `Ordering::Greater`,
   4. `>=` is true exactly for `Ordering::Greater` or `Ordering::Equal`.
7. Types MUST NOT override the four relational operators independently.
8. An inherent method, a local trait named `Ord`, or any intrinsic not identified as `std::prelude::Ord` MUST NOT alter operator dispatch.
9. If operand compatibility or the required canonical `Ord` implementation is absent, compilation MUST fail with `COMPARISON_TYPE_MISMATCH`.
10. Relational comparison results have type `Bool`.
11. For floating scalar values, runtime/evaluation semantics are defined by the unified floating total-order rules in `../stdlib/conventions.md`.
12. Numeric promotion that produces `complex64` or `complex128` fails Rule 4 because complex scalar types do not implement `Ord`.

### Type Checker vs Tensor Constraint Boundary

The general type checker and tensor constraint system are adjacent layers under
`implementation-boundary.md`:

1. the general type checker,
2. the tensor constraint system.

Responsibilities are normative and MUST remain separated.

General type checker responsibilities:

1. MUST type-check all non-tensor language forms (`fn`, generics, traits/coherence, effects, control flow, `Res/Opt` typing) without requiring tensor dim-solve execution.
2. MUST type-check tensor API signatures at the type-constructor level (`Tensor[T]`, `Res[...]`, `Opt[...]`, `AxisSel`, and related declared types).
3. MUST enforce the declared arity of tensor-related constructors. `Tensor` and `DType` each require exactly one type argument in source type positions; bare `Tensor` or bare `DType` MUST fail with `TYPE_ARG_ARITY_MISMATCH`.
4. MUST infer a cast destination type argument `U` from an ordinary descriptor argument of type `DType[U]`; successful tensor cast calls therefore have result payload type `Tensor[U]`.
5. MUST NOT synthesize an erased common type for distinct `Tensor[A]` and `Tensor[B]` or for distinct `DType[A]` and `DType[B]`. Ordinary branch/arm common-type diagnostics apply when `A` and `B` differ.
6. MUST NOT use tensor axis names, runtime shapes, runtime index values, or backend/lowering plan details as additional type-level unification or trait-coherence discriminators.
7. MUST produce compile-time diagnostics for statically decidable declaration/expression typing failures.
8. MUST NOT consult tensor schema, axis, shape, dtype-value, lowering, or backend
   facts to resolve an ordinary non-tensor name, call, overload, generic
   argument, trait obligation, branch result, effect, or ownership state.

Tensor constraint system responsibilities:

1. MUST validate tensor transform/indexing constraints (axis expression validity, dim solving, shape/broadcast compatibility, indexing legality) under tensor-model rules.
2. MUST produce compile-time diagnostics when a tensor constraint failure is statically decidable from source/literals.
3. MUST produce runtime `Err` results (not compile-time rejection) when success/failure depends on runtime values.
4. MUST keep success/failure classification invariant under backend/lowering optimization choices.
5. MUST be invoked only for a tensor activation surface defined in
   `implementation-boundary.md`.

Implementations MAY constant-fold and report earlier only when doing so is semantics-preserving and does not change required diagnostic class for non-constant executions.

Compile-time vs runtime channel selection and per-operation classification are normatively defined in `validation-matrix.md`.
