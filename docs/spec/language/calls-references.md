# Calls and Reference Semantics

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Evaluation Order

Unless otherwise specified, expression evaluation order MUST be left-to-right.

Any API with fail-fast behavior MUST define a deterministic stop point in this order.


### Call Semantics

Default parameter passing semantics are by-value for ownership-free values.

Implementations MAY optimize with move, borrow, or copy-on-write techniques if observable behavior is preserved.

`move` is a language operator from `ownership-boundary.md`, not a function call surface.
When ownership surface is disabled, ownership syntax is rejected with `OWNERSHIP_PROFILE_DISABLED`.

Named bindings of ownership-root type are the explicit exception to default by-value copying:

1. transfer into a by-value ownership-root parameter requires `move <identifier>`,
2. a fresh ownership-bearing call result may bind directly because it has no prior named root,
3. use of a named root in a consuming call position without `move` fails with `OWNERSHIP_TRANSFER_REQUIRES_MOVE`,
4. reference parameters borrow roots without transferring them.

Value transfer and assignment semantics are by-value by default:

1. For ownership-free values, `a = b` MUST preserve value semantics (no required observable aliasing between `a` and `b` after assignment).
2. Implementations MAY realize ownership-free assignment using move/borrow/COW techniques only if Rule 1 remains observationally true.
3. Explicit reference values are an exception to Rule 1: copying a reference
   copies its referent-place identity, not the referenced value. Every copy
   remains an observable alias under the reference rules below.
4. Within a raw-pointer relay region, each permitted immutable relay binding
   copies the same foreign address identity. This is a second, narrower
   exception governed entirely by `unsafe-ffi.md`; it does not authorize
   pointer assignment, storage, escape, or use outside that region.
5. Ownership roots are not replaceable assignment targets in v0.1; replacing one would discard its prior obligation and MUST fail with `OWNERSHIP_DISCARD_FORBIDDEN`.

Opaque mutable resource payloads are not exposed as ownership-free values:

1. A standard-library acquisition API that transfers an external resource MUST return an allowed root shape such as `Res[Own[R], E]`.
2. Resource operations borrow the wrapper explicitly as `&Own[R]` or `&mut Own[R]`; ordinary method lookup MUST NOT unwrap `Own[R]` to `R`.
3. A standard-library module MAY declare methods directly on a specific `Own[R]` instantiation.
4. A consuming method on a named resource root uses the ordinary explicit transfer surface, for example `(move reader).close!()`.
5. Parentheses in Rule 4 are required by the v0.1 expression grammar because `move reader` is a unary expression and postfix method application begins from a primary expression.

Assignable target semantics:

1. The discard target `_ = expr` is assignable only when `expr` has an ownership-free type.
   Discarding an ownership-bearing expression, including `_ = move x`, MUST fail with `OWNERSHIP_DISCARD_FORBIDDEN`; use `drop_own move x` for an ownership root.
2. Identifier assignment is assignable only when the identifier resolves to an existing mutable local binding in the current scope.
3. Bare assignment to an unresolved identifier MUST NOT introduce a local binding in v0.1 and MUST fail with `ASSIGN_TARGET_NOT_ASSIGNABLE`.
4. Stored-field assignment `recv.name = expr` is assignable only for declared `field mut` members; immutable fields and properties use the dedicated diagnostics in `bindings-structs.md`.
5. Explicit dereference assignment `*r = expr` is assignable only when `r` has type `&mut T` and `expr` is assignable to `T`.
6. Call results, `?` expressions, and other value-producing `PostfixExpr` forms are not assignable targets and MUST fail with `ASSIGN_TARGET_NOT_ASSIGNABLE`.
7. General index expressions and index-assignment sugar are not part of v0.1; use explicit access or update APIs such as `get`, `set!`, `get_at`, `set_at`, or `set_at!` where provided.


### Named and Trailing Arguments

v0.1 call surface includes positional arguments, named arguments, and trailing closure arguments.
Named argument availability depends on the callee's exposed parameter names; calls through `Fn` values follow the restriction in Callable Type Compatibility Rule 9.

Rules:

1. Calls MAY mix positional and named arguments.
2. Positional arguments MUST appear before named arguments (`CALL_POSITIONAL_AFTER_NAMED` otherwise).
3. Each callee parameter MUST be bound at most once.
4. Unknown named arguments (`CALL_UNKNOWN_NAMED_ARG`), duplicate named bindings (`CALL_DUPLICATE_NAMED_ARG`), and positional-after-named usage (`CALL_POSITIONAL_AFTER_NAMED`) MUST be compile-time errors.
5. Trailing closure arguments are call sugar:
   1. `callee { |x| body }` desugars to `callee({ |x| body })`.
6. `std::concurrency::spawn!` additionally admits a dedicated trailing thunk block:
   1. `spawn! { body }` desugars to `spawn!({ || body })`,
   2. this thunk has no parameters,
   3. its body uses closure block tail-expression semantics,
   4. this rule applies only after name resolution identifies the callee as `std::concurrency::spawn!`,
   5. for resolved `std::concurrency::spawn!`, this rule takes precedence over placeholder-closure recognition for a trailing `{ Expr }` block without an explicit parameter list,
   6. after any other callee, a trailing non-lambda block is not a valid call argument surface and MUST NOT be accepted through ordinary argument binding.
7. Named argument support is language-wide in v0.1 and is not restricted to selected intrinsics.


### Explicit Call-Site Type Arguments

v0.1 includes a minimal explicit generic-call escape hatch.

Rules:

1. Explicit type arguments use `callee::[T0, T1, ...](...)` surface.
2. This surface is valid on path-like and member-like callees.
3. Explicit type arguments are immediate-call only:
   1. `id::[i64](7)` is baseline syntax,
   2. `id::[i64]` as a standalone function value is not part of v0.1 baseline.
4. Explicit type-argument lists are all-or-none for the callee generic arity:
   1. partial explicit lists are not part of v0.1 baseline,
   2. a partial explicit list MUST fail with `TYPE_ARG_INFERENCE_FAILED`.
5. Trailing closure call sugar remains valid after explicit type arguments when the underlying call surface is valid.
6. If omitted type arguments remain underconstrained, compilation still fails with `TYPE_ARG_INFERENCE_FAILED`.
7. Bracket-only generic-call attempts such as `callee[T](...)` are not explicit call-site type arguments in v0.1 and MUST fail with `TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN`; parser recognition of the bracket-call shape does not grant ordinary callees this surface.
8. Rule 7 does not apply when the callee resolves to the canonical identity of a
   constructor that owns the dedicated `std::tensor` dtype-bracket surface, such
   as `ones[f32](...)`. Import aliases and re-exports preserve that identity;
   source spelling alone never grants the exception.
9. Both inferred and explicit type arguments MUST satisfy every inline trait bound declared by the callee (`TRAIT_BOUND_NOT_SATISFIED` otherwise).
10. Explicit argument arity includes all declared generic parameters, whether bounded or unbounded; bounds do not add hidden type arguments.


### Closure Literals

Closure literals use explicit `{ |...| ... }` surface or placeholder shorthand (`{ _.field }`, `{ f(_) }`) and are value expressions.

Rules:

1. Parameter list uses bindings or non-binding `_` discards separated by commas:
   1. unannotated examples: `{|x| ...}`, `{|a, b| ...}`,
   2. annotated examples: `{|x: i64| ...}`, `{|a: i64, b| ...}`,
   3. discard examples: `{|_| ...}`, `{|_: i64, x| ...}`,
   4. the pipe-delimited parameter list is line-local; it does not inherit the
      paired-delimiter comma-list newline policy from `syntax.md`.
2. Closure body forms:
   1. single-expression form (`{|x| x + 1}`),
   2. block form with statements and optional final tail expression.
3. In closure block form, normal completion of the final tail expression (if
   present) creates the closure result edge and carries that expression's value.
4. If closure block form omits a tail expression and reaches normal fallthrough,
   its result type is `Unit` and value is `()`. If the body has no normal result
   edge, including when a present tail expression cannot complete, its result
   type is `Never`; this does not introduce an implicit tail expression.
5. Closure block form is `LambdaBody` syntax, not `BlockExpr`.
   Its final tail-expression rule is a closure-specific carve-out and MUST NOT be applied to ordinary block expressions or function/method block bodies.
6. Closure bodies are control-flow boundaries for `return`, `break`, and `continue` as defined in `control-flow.md`.
7. Effect legality of closure bodies is checked in the enclosing call context:
   1. pure combinators require pure closures,
   2. bang combinators may accept effectful closures.
8. Closure parameter annotations are part of v0.1 baseline:
   1. each binding or discard parameter may optionally include `: TypeExpr`,
   2. mixed annotated/unannotated parameter lists are allowed,
   3. closure return-type annotations are not part of v0.1 baseline.
9. Placeholder closure shorthand (`{ _.field }`, `{ f(_) }`) is part of the v0.1 closure surface.
10. Placeholder closure shorthand MUST contain at least one `_` hole expression.
11. Placeholder parameters are introduced in left-to-right hole occurrence order, using the same ordering rule as hole-based partial application.
12. A `_` hole expression is valid in v0.1 only when it appears within:
   1. a placeholder closure shorthand body, or
   2. a call expression that is being lowered as hole-based partial application.
13. A `_` hole expression outside those contexts MUST fail with `PLACEHOLDER_INVALID_CONTEXT` before ordinary expression type inference or operator typing.
14. This rule applies only to `_` parsed as a `HoleExpr`; wildcard patterns and discard assignment targets are not placeholder holes.
15. Explicit closure parameters are introduced simultaneously in one parameter
    scope and are active throughout the closure body. Repeating a parameter
    name in that scope MUST fail with `LOCAL_BINDING_REDECLARATION`. A `_`
    parameter introduces no binding, is not a placeholder hole, and MAY repeat.
16. The closure `LambdaBody` introduces a local scope nested inside the
    parameter scope. A closure-local binding MAY therefore shadow a closure
    parameter under `bindings-structs.md`.


### Closure Capture Semantics

A captured local is a free value identifier in a closure body that resolves to
an enclosing function, method, or closure parameter, local binding, or pattern
binding.
Module constants, named functions, and bindings introduced inside the same
closure are not captured locals.

Rules:

1. The capture set is determined statically after lexical name resolution over
   the complete closure body. Each identifier use resolves to the nearest
   binding active at that source position under `bindings-structs.md`; a later
   closure-local declaration is not hoisted and cannot retroactively change an
   earlier outer-binding use into a local use.
   A reference in an unreachable or statically false branch still participates
   in capture validation.
2. An ordinary value capture MUST resolve to an immutable binding whose type is
   ownership-free and reference-free.
3. Each ordinary captured value is read exactly once when the closure expression
   is evaluated and stored as an immutable closure-environment value.
4. A `let mut` binding MUST NOT be captured, even when the closure only reads
   it (`MUTABLE_CAPTURE_FORBIDDEN` otherwise).
   Code that intentionally freezes its current value MUST first copy it into a
   separate immutable `let` binding and capture that binding.
5. A closure MUST NOT assign to a captured binding or mutate storage rooted in
   a captured binding (`MUTABLE_CAPTURE_FORBIDDEN` otherwise).
   Closure-local bindings and storage created during an invocation remain
   eligible for the local-only mutation rule in `effects.md`.
6. Later rebinding or mutation through another ownership-free copy MUST NOT
   change the captured value.
7. Repeated invocation cannot mutate the captured environment.
   Copying, rebinding, passing, returning, or storing a closure with only
   ordinary captures preserves ownership-free value semantics and MUST NOT
   create observable shared capture state.
8. Ordinary immutable captures MAY escape their defining scope.
9. An immutable binding whose value is an explicit reference MAY be captured
   only by a non-escaping closure:
   1. capture does not extend the referent lifetime,
   2. an escaping closure fails with `REF_ESCAPE_FORBIDDEN`,
   3. mutation through a captured `&mut T` fails under Rule 5 before escape
      classification can authorize it.
10. Capturing an ownership root or a reference derived from one remains
    forbidden with `OWNERSHIP_TYPE_POSITION_FORBIDDEN` under
    `ownership-boundary.md`.
11. Rules 4-5 apply to free-variable capture by closure literals, including
    placeholder closure shorthand.
    They do not change the explicit, exactly-once evaluation of non-hole
    argument expressions in a hole-based partial application.
12. The capture contract applies regardless of whether the receiving API
    authorizes an effectful callback body.
    Effect authorization does not permit mutable capture state.
13. Capture validation precedes callable compatibility, ordinary assignment
    diagnostics, and effect checking.
    When more than one capture failure applies:
    1. `OWNERSHIP_TYPE_POSITION_FORBIDDEN` precedes
       `MUTABLE_CAPTURE_FORBIDDEN`,
    2. `MUTABLE_CAPTURE_FORBIDDEN` precedes
       `IMMUTABLE_BINDING_ASSIGN` and `REF_ESCAPE_FORBIDDEN`,
    3. failures of the same code select the first offending capture use in
       source order.


### Callable Type Compatibility

`Fn(P0, ..., Pn) -> R` is the only surface callable type in v0.1.
`Fn` is a reserved built-in callable type head, not an ordinary prelude type alias or user-definable type name.

Rules:

1. `Fn(...) -> ...` denotes a pure callable.
   v0.1 does not define a surface effectful callable type such as `Fn!(...) -> ...`; that remains deferred by the open-items ledger.
2. A callable value is compatible with expected type `Fn(P0, ..., Pn) -> R` only when:
   1. its arity is exactly the expected `Fn` parameter count,
   2. for each parameter position `i`, the expected argument type `Pi` is assignable to the source callable parameter type,
   3. its result type is assignable to `R`,
   4. invoking it does not require an effectful/bang callable.
3. A named non-bang function value is compatible with `Fn(P0, ..., Pn) -> R` when Rule 2 holds after overload resolution and generic instantiation.
   If an overload set is used as a function value, expected `Fn` type context MUST select exactly one compatible overload. Multiple compatible overloads fail with `OVERLOAD_AMBIGUOUS`; no compatible overload fails with `CALLABLE_VALUE_NO_MATCH`, including when the value appears as a bound call argument.
   Callable-value selection uses the complete assignability relation from
   Rule 2, including `Res` error-union assignability. Declaration-time overload
   disjointness uses exact call applicability instead, so a legal overload set
   can still contain multiple members compatible with one expected `Fn`; that
   coercion fails with `OVERLOAD_AMBIGUOUS`.
   Declared trait bounds MUST NOT remove or rank candidates whose instantiated
   callable types satisfy Rule 2. If multiple such candidates remain, coercion
   MUST fail with `OVERLOAD_AMBIGUOUS` even when an inferred type argument of
   one candidate would fail its bound. Only after exactly one compatible
   candidate remains are its declared bounds checked. A bound failure on that
   selected candidate MUST fail with `TRAIT_BOUND_NOT_SATISFIED`; it MUST NOT
   fall back to another overload.
   A generic named function is monomorphized at this coercion and does not create a polymorphic function value. If no expected `Fn` type fully determines its type arguments, the function-value expression MUST fail with `TYPE_ARG_INFERENCE_FAILED`.
   An expected `Fn` component matched only against a projection such as `T::Item` does not determine `T`. Because a standalone function-value specialization expression such as `f::[Concrete]` without a call is outside v0.1, a projection-only generic parameter can participate in an immediate explicitly typed call but cannot be specialized into a function value without another non-projected position that determines it.
   A fully qualified non-bang trait method reference is a named function value
   after its concrete `Self` is determined from the expected receiver component
   under `traits.md`; later associated projections do not reverse-infer
   `Self`.
4. A closure literal, including placeholder closure shorthand, is compatible with `Fn(P0, ..., Pn) -> R` when:
   1. its parameter count is exactly the expected `Fn` parameter count,
   2. unannotated parameter types are inferred from the expected `Pi` positions,
   3. annotated parameter types are compatible with the expected `Pi` positions,
   4. its body result type is assignable to `R`,
   5. its captures satisfy the closure capture contract above,
   6. its body is pure under `effects.md`.
5. A hole-based partial application value is compatible with `Fn(P0, ..., Pn) -> R` when:
   1. the partial application has exactly the expected `Fn` parameter count of holes,
   2. hole parameter types in left-to-right order are compatible with the expected `Pi` positions,
   3. non-hole arguments are checked and captured exactly once at partial-construction time,
   4. the target invocation result type is assignable to `R`,
   5. the target invocation is pure.
6. Supplying an effectful closure, bang named function, or bang-target partial application where a pure `Fn(...) -> ...` is required MUST fail with `EFFECT_VIOLATION`.
   Pure higher-order combinator contracts continue to use `PURE_COMBINATOR_EFFECTFUL_CALLBACK` as specified in `effects.md`.
7. If callable compatibility fails because of arity, parameter type, or result type at a call argument binding, compilation MUST fail with `CALL_ARG_TYPE_MISMATCH` unless a more specific diagnostic from this section applies.
8. A `Fn(...) -> ...` value is an escaping value when it is assigned to a `Fn`-typed binding, returned, stored, or passed as an argument.
   Capturing an explicit reference inside a closure or partial application that becomes such an escaping `Fn` value MUST fail with `REF_ESCAPE_FORBIDDEN`.
9. Calling a value whose static type is `Fn(P0, ..., Pn) -> R` follows ordinary positional call argument typing rules from `type-system.md`.
   `Fn` types do not expose parameter names, so any named argument in such a call MUST fail with `CALL_UNKNOWN_NAMED_ARG`, even if the value's underlying function declaration has a parameter with that spelling.
   Calling a function declaration directly retains its named-call surface.
10. v0.1 bang APIs that accept effectful callbacks MUST specify those callback contracts directly in their API rules; they are not expressed as a surface `Fn!` type.
11. A closure or partial application that captures an ownership root or a reference derived from one is an ownership-capable callable value and MUST fail with `OWNERSHIP_TYPE_POSITION_FORBIDDEN` under `ownership-boundary.md`.
12. Rule 11 takes precedence over `REF_ESCAPE_FORBIDDEN` when the same capture is both ownership-bearing and reference-escaping.
13. `Fn(...) -> ...` is a runtime-sized callable handle for recursive-layout analysis; its parameter and result types do not contribute inline storage edges.
14. A scoped standard-library resource callback MAY receive `&mut Own[R]` as an API-level parameter when the API rules require exactly one non-escaping invocation.
    This parameter is not a captured root, is not a source-level `Fn` type position, and MUST NOT be stored, returned, or captured by another escaping callable.
15. An expected `Fn` type MAY reach a callback expression through the closed
    expression-local schema specialization defined by
    `../stdlib/catalog-signatures.md`. Only a listed adapter invocation that is
    complete in the same expression may supply that expected type; canonical
    `then` and unary-tuple `apply` count after desugaring to the listed direct
    invocation. No expected type may flow backward through a local binding.
    Named functions, overload sets, placeholder closures, and partial
    applications still require ordinary construction-site specialization.
16. A callable whose exact result type is `Never` is compatible with any
    independently established expected result `R` under the directional
    assignability rule in `type-system.md`. The reverse does not hold, parameter
    compatibility is unchanged, and this rule supplies no automatic trait or
    effect capability.
17. `Fn(...) -> ...` does not implement `Eq` in v0.1; callable identity or behavior equality is not observable through `==` or `!=`.


### Local Reference Values

The language MUST support explicit reference notation:

1. `&T` for shared read-only reference.
2. `&mut T` for writable reference.

Neither form is nullable, pointer-arithmetic-capable, or an ownership root.
`&mut T` denotes write capability through that reference; it does not denote a
unique or exclusive borrow.

Reference target rules:

1. A **stable local place** is invocation-local storage rooted in a function or
   method parameter or an initialized local binding, followed by zero or more
   stored-field or tuple projections, or reached by dereferencing a valid local
   reference.
2. A property read, call result, literal, constructor temporary, arithmetic or
   control-flow result, module/const binding, and other value-only expression is
   not a stable local place. General index places are not part of v0.1.
3. `&place` requires a readable stable local place. `&mut place` requires a
   writable stable local place under the ordinary binding, stored-field, and
   dereference assignment rules. An immutable reference binding may still be
   dereferenced for writes when its value has type `&mut T`; binding mutability
   controls rebinding the reference value, not writing its referent.
4. An invalid reference operand MUST fail with `REFERENCE_TARGET_INVALID`.
   Name resolution and operand typing precede this target classification.
   Reference-target validation precedes storage-containment and escape
   classification for the same reference expression.
5. A reference designates its stable place, not a snapshot of the place's
   current value. Assignment through the original place, an enclosing stored
   place, or any writable alias is immediately observable through every other
   alias.

Reference alias rules:

1. `&T` prevents writes only through that reference. It does not freeze the
   referent against writes through the original place or another `&mut T`.
2. Reference initialization, copying, and assignment preserve referent-place
   identity. Copying an `&mut T` does not invalidate the source reference and
   does not require `move`.
3. Assigning to a mutable reference binding replaces the reference value held
   by that binding. Assigning through `*reference` updates the referent instead.
4. `&*reference` creates a read-only alias to the same place. `&mut *reference`
   creates a writable alias when `reference` has type `&mut T`.
5. `&mut T` is assignable to `&T` for the same `T` by access weakening; the
   resulting reference remains an alias to the same place. `&T` is not
   assignable to `&mut T`.
6. Shared and writable references to one place MAY coexist and MAY be used in
   any left-to-right source order. v0.1 performs no exclusivity, overlap,
   last-use, region, or borrow-conflict analysis.
7. Distinct call arguments, including a receiver and another argument, MAY
   designate the same place through any mixture of `&T` and `&mut T`.
   The callee observes ordinary left-to-right execution against that one place
   and MUST NOT assume its reference parameters are disjoint.
8. Implementations MUST NOT attach a persistent `noalias` assumption merely
   because a value or parameter has type `&mut T`. They MAY recover disjointness
   from independent analysis when doing so preserves these observable rules.

Reference storage and escape rules:

1. Parameter storage lasts for the invocation. Initialized local-binding
   storage begins after its initializer and lasts until its containing lexical
   block exits on the current path. A stored projection has the storage extent
   of its root place; a dereferenced place inherits every possible referent
   extent carried by the reference value.
2. A local binding or local tuple value that contains a reference MUST have a
   lexical storage extent contained within every possible referent's storage
   extent. The same containment check applies at each reference-valued
   assignment and control-flow merge.
3. Reference copies, rebindings, tuple components, and control-flow merges
   preserve the finite set of possible referent places. This is lexical
   containment and finite provenance propagation, not lifetime inference.
4. A direct reference argument exists through the complete callee invocation.
   Passing a reference derived from an ownership root therefore remains a use
   of that root through the call for ownership-flow purposes.
5. Returning a reference value, storing one in a struct field or other escaping
   storage, capturing one in an escaping closure, or assigning one into local
   storage that can outlive any possible referent is forbidden.
6. Violations of Rule 5 MUST fail with `REF_ESCAPE_FORBIDDEN`. The primary span
   is the return, store, capture, or reference-valued assignment that first
   crosses the permitted storage extent in source order.
7. A non-escaping closure MAY copy an immutable reference binding under the
   closure-capture rules above. That copy remains an alias and does not extend
   any referent's storage extent.
8. Introducing a same-spelled binding in a nested scope creates separate
   storage. It does not shorten the outer binding's storage extent, retarget an
   existing reference, or alter any referent or ownership-root provenance.

Reference signature and ownership rules:

1. Method receiver shape and `Self` scope are defined by `Method Receivers and Self` below.
2. A receiver-borrowed bang method MUST use `&mut Self`, not shared `&Self` (`EFFECT_SIGNATURE` otherwise).
3. A non-bang method MUST NOT take `&mut Self` (`EFFECT_SIGNATURE` otherwise).
4. Violations of Rules 2-3 are declaration-signature errors and MUST be diagnosed before ordinary receiver-body typing so that the primary compile-time code remains `EFFECT_SIGNATURE`.
5. A call requiring `&mut T` MUST receive a writable reference, either explicit
   or type-checker inserted. Insertion grants no uniqueness or disjointness.
6. A non-bang function or method MUST NOT declare non-receiver parameters of type `&mut T` (`NON_BANG_MUT_PARAM_FORBIDDEN` otherwise).
7. Explicit dereference uses `*r`:
   1. reading through `*r` is valid when `r` has type `&T` or `&mut T`,
   2. assignment through `*r = expr` is valid only when `r` has type `&mut T` and `expr` is assignable to `T`,
   3. assignment through a shared `&T` reference is forbidden under ordinary assignment/type checking.
8. A local reference derived from an ownership root retains root provenance under `ownership-boundary.md`; a later use after that root is consumed MUST fail with `OWNERSHIP_USE_AFTER_DISCHARGE`.
9. The ownership rule in Rule 8 is use-sensitive: moving the root after the derived reference's last use is valid. This ownership-root availability check does not create general reference exclusivity.
10. `&mut Own[T]` is an aliasable writable reference and does not prove unique
    ownership. An API that requires unique exact-once responsibility MUST take
    `Own[T]` by value and receive a named root through explicit `move`.
11. Reference types implement neither `Eq` nor pointer-identity comparison in v0.1. Equality MUST NOT implicitly dereference references; comparing two references fails with `COMPARISON_TYPE_MISMATCH`.


### Reference-Free Abstraction Slots

v0.1 keeps borrows visible at the type boundary that introduces them rather than allowing a generic or associated type slot to conceal them.

1. A **reference-free type** contains no `&T` or `&mut T` reference node after transparent alias expansion and normalization of every associated projection that is currently resolvable.
2. Reference-free validation recursively visits the complete type expression, including tuple elements, generic arguments, and `Fn` parameter and result types.
3. A canonical associated projection that remains symbolic is reference-free by the associated-slot invariant in `traits.md`; every concrete binding for that slot is validated separately.
4. The restriction applies to ordinary generic arguments and associated type bindings, not to every source type position. Direct reference positions remain governed by the non-escaping rules above.
5. Code that borrows an abstract value MUST write the borrow outside the abstraction slot, for example `&T`, `&mut T`, `&T::Item`, or `&mut T::Item`, where the surrounding reference and effect rules permit that form.
6. A reference-bearing abstraction argument or associated type binding MUST fail with `REFERENCE_TYPE_POSITION_FORBIDDEN`.


### Method Receivers and `Self`

v0.1 methods are receiver-bearing callables declared in struct bodies, trait bodies, or trait impl bodies.

Rules:

1. A callable declaration in a struct body, trait body, or trait impl body MUST declare a receiver parameter first.
2. Associated/static methods without a receiver parameter are not part of the v0.1 baseline.
3. The receiver parameter name MUST be exactly `self`.
4. No non-receiver parameter may be named `self`.
5. The receiver parameter type in source declarations MUST be one of:
   1. `Self`,
   2. `&Self`,
   3. `&mut Self`.
6. Standard-library declarations use the closed [catalog signature metalanguage](../stdlib/catalog-signatures.md); an owner-qualified declaration such as `fn Vec[T].push(self: Vec[T], x: T) -> Vec[T]` binds the receiver to that catalog owner, while source declarations inside an owner use `Self`.
7. `Self` denotes the owning struct type in struct bodies and inherent methods, and the eventual implementing type in trait declarations and trait impl items.
8. `Self` is valid in receiver types, non-receiver parameter types, return types,
   and method bodies while checking a struct-body method, trait method, or trait
   impl method; it is also valid in an associated-type binding RHS while checking
   that impl. This scope rule does not itself prove a trait obligation:
   `traits.md` separately defines the exact current-trait assumption available
   to a default body.
9. `Self` in top-level function signatures, top-level type aliases, local annotations outside owner-bound method bodies, struct field/property types, or extern signatures is invalid in v0.1 (`SELF_TYPE_INVALID`).
10. Missing receiver parameters, non-first receiver parameters, receiver names other than `self`, receiver types outside Rule 5, or duplicate `self` parameters MUST fail with `METHOD_RECEIVER_INVALID`.
11. Receiver bangness and mutability are checked after receiver-shape validation:
    1. non-bang methods MAY use `self: Self` or `self: &Self`,
    2. non-bang methods MUST NOT use `self: &mut Self` (`EFFECT_SIGNATURE`),
    3. bang methods MAY use `self: Self` or `self: &mut Self`,
    4. bang methods MUST NOT use shared borrowed receiver `self: &Self` (`EFFECT_SIGNATURE`).


### Postfix Application and Partial Application

`then` and `apply` are language intrinsics and MUST be desugared as:

1. `x.then(f)` -> `f(x)`
2. `(a, b, c).apply(f)` -> `f(a, b, c)`

These rewrites MUST be type-independent and MUST NOT introduce additional effects beyond the desugared call.

`then` and `apply` are alternative call syntax, not higher-order combinator
APIs. The supplied callable becomes the desugared call target and retains its
canonical identity, bang marker, and effect summary. Consequently,
`x.then(f!)` is effect-equivalent to the visibly bang call `f!(x)` and is
checked in exactly the same context.

No `then!` or `apply!` forms are required: a bang marker on the sugar name
would duplicate or obscure the marker on the actual callee without authorizing
anything new. This differs from `tap` / `tap!` and other callback-taking APIs.
Those APIs own callback invocation and therefore expose separate
pure/effectful contracts under `effects.md`; `then` and `apply` own no callback
execution beyond the ordinary call produced by the rewrite.

Diagnostics for failures arising from these rewrites MUST remain source-oriented:

1. The primary source span MUST reference the original sugar surface syntax
   (`then`/`apply`/hole partial form), not only the lowered internal call node.
2. Implementations MAY attach lowered internal locations as supplemental related spans/notes.

The `_` token in call position denotes hole-based partial application:

1. If a call expression has `k` holes, it yields an uncurried callable of arity `k`.
2. Hole parameters are ordered left-to-right.
3. Non-hole argument expressions MUST be evaluated exactly once at partial-construction time and captured.
4. A bare `_` expression, `_` used as an initializer, and `_` used inside an ordinary non-placeholder expression such as `_ + 1` are not partial application forms and MUST fail with `PLACEHOLDER_INVALID_CONTEXT`.
5. `_ = expr` uses the discard assignment target surface and is not a hole-based partial application form.
