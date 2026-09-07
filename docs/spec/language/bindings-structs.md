# Bindings and Struct Semantics

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Lexical Local Scopes and Binding Activation

These rules govern value bindings introduced by callable and closure parameters,
local `let` patterns, `for-in` binders, and `match` patterns. Type parameters,
module declarations, and member namespaces keep their separately specified
scopes.

Rules:

1. Every local value binding has a distinct semantic identity. Unqualified local
   lookup searches active lexical scopes from innermost to outermost and selects
   the first matching binding.
2. Every ordinary `BlockExpr` introduces a lexical local scope. This includes
   function and method bodies, bare blocks, conditional branches, block-valued
   match arms, loop bodies, and `result`, `checked`, and `unsafe` bodies.
3. A function, method, trait method, or extern declaration introduces one
   parameter scope containing all of its value parameters. That scope encloses
   the complete body of a callable that has one. A body `BlockExpr` introduces
   a nested scope, so a body-local binding MAY shadow a parameter; an expression
   body resolves directly in the parameter scope.
4. A closure introduces one parameter scope containing all explicit or inferred
   closure parameters. Its `LambdaBody` introduces a nested local scope, so a
   closure-local binding MAY shadow a closure parameter.
5. A `for-in` iterable expression is resolved and evaluated in the surrounding
   scope. The loop binder is then initialized once per iteration in a binder
   scope that encloses the loop-body `BlockExpr`; the body block is nested and
   MAY shadow the binder. The binder is not visible in the iterable expression
   or after the loop.
6. The valid bindings introduced by one `match` pattern form one arm scope. They
   are active in that arm's guard and result expression, but not in another arm
   or after the `match`. A block-valued arm result introduces a nested
   `BlockExpr` scope and MAY shadow an arm binding.
7. Before checking a local initializer, a compiler MUST determine the names the
   accepted binding pattern would introduce and reject any same-scope collision.
   The declaration being checked does not become active for this step.
8. A local initializer is resolved and checked with every binding from that
   declaration absent. A same-spelled active outer binding therefore remains
   visible in a valid nested shadowing initializer; without such an outer
   binding, a self-reference fails with `NAME_NOT_FOUND`. No binding in a tuple
   or other binding group is visible anywhere in that declaration's
   initializer.
9. After the initializer, annotation, and pattern checks succeed, every binding
   in the declaration becomes active atomically and remains active through the
   rest of its containing lexical scope. Local declarations are not hoisted.
10. A nested lexical scope MAY introduce a binding with the same spelling as an
    outer local binding, parameter, loop binder, or match binding. It hides that
    outer binding only while the nested binding is active.
11. A second binding with the same spelling in one lexical scope MUST fail with
    `LOCAL_BINDING_REDECLARATION`. This includes separate local declarations in
    one block and repeated names in one callable or closure parameter list.
12. A duplicate identifier inside one local `let` pattern remains the more
    specific `LET_PATTERN_DUPLICATE_BINDING`. Valid or-pattern alternatives that
    establish one common arm binding set do not count as redeclarations.
13. Same-scope collision validation occurs before initializer name resolution,
    type inference, effect checking, and ownership analysis. Syntax,
    reserved-name, parameter-shape, unsupported-pattern, and intra-pattern
    duplicate diagnostics retain their more specific precedence. The primary
    span for `LOCAL_BINDING_REDECLARATION` MUST identify the second introduction
    of the first colliding name in source order.
14. Shadowing changes spelling lookup only. It MUST NOT replace a binding's
    identity, shorten storage or reference provenance, consume an ownership
    root, discharge an ownership obligation, or merge mutable state between the
    outer and inner bindings.

### Binding and Struct Member Mutability

Mutability of local bindings and struct members is explicit.
Stored fields hold data.
Properties name accessor-backed API.
`mut` permits a write surface, but writable properties still use explicit setter calls rather than assignment syntax.

Rules:

1. `let` declares an immutable local binding.
2. `let mut` declares a mutable local binding.
3. Assignment to an immutable `let` binding MUST fail with `IMMUTABLE_BINDING_ASSIGN`.
4. Assignment to a `let mut` binding requires the assigned value to be
   assignable to the binding type (`ASSIGNMENT_TYPE_MISMATCH` otherwise).
5. Assignment to an unresolved identifier MUST NOT introduce a local binding; local bindings are introduced only by declarations and binding patterns (`ASSIGN_TARGET_NOT_ASSIGNABLE` otherwise). An identifier assignment targets the nearest active local binding selected by the lexical-scope rules above.
6. Stored struct members are declared with `field` / `field mut`.
7. Property members are declared with `prop` / `prop mut`.
8. Assignment to an immutable `field` member MUST fail with `IMMUTABLE_BINDING_ASSIGN`.
9. `prop` declarations are read-only property surfaces.
10. `prop mut` declarations are writable property surfaces, but writable access remains explicit through `set_<name>!` rather than `recv.<name> = value`.
11. `var` is reserved and MUST be rejected in all declaration positions in v0.1 (`VAR_DECL_FORBIDDEN`).
12. Any declaration use of `var` (for example `var x = ...`, `struct C { var n: i64 }`, or `trait T { var n: i64 }`) MUST fail with `VAR_DECL_FORBIDDEN`.
13. `VAR_DECL_FORBIDDEN` is phase-invariant:
   1. implementations MAY detect at parse or later static phase,
   2. emitted compile-time code MUST remain `VAR_DECL_FORBIDDEN`.
14. `let mut` grants mutation only within its lexical function or closure
    region; it does not grant closure-capture permission.
    Capturing such a binding fails with `MUTABLE_CAPTURE_FORBIDDEN` under
    `calls-references.md`.
15. The confined raw-pointer relay in `unsafe-ffi.md` is a narrower local-binding rule:
    1. only an unannotated immutable identifier pattern `let name = <expr>` may introduce a relay local,
    2. `let mut` or a destructuring/binding pattern that would retain the pointer MUST fail with `RAW_POINTER_USE_UNSUPPORTED`,
    3. an explicit raw-pointer annotation is a source-written non-extern type position and MUST fail with `RAW_POINTER_TYPE_UNSUPPORTED`,
    4. `let _ = <expr>` introduces no binding and remains an allowed relay discard.

### Struct Construction and Field Initialization

v0.1 separates stored fields from properties.
Struct literals initialize stored fields only.
Properties are not initialized because they do not store data.

Rules:

1. Stored field declarations are type-only in v0.1 (`field name: Type` or `field mut name: Type`).
2. Field default initializer syntax is not part of accepted v0.1 surface, but parse-valid `field name: Type = expr` forms MUST be rejected with stable code `STRUCT_FIELD_DEFAULT_FORBIDDEN`.
3. A `struct` declaration with an inheritance clause (`struct Name : TypeExpr { ... }`) is parse-valid diagnostic surface only and MUST fail during declaration validation with `INHERITANCE_FORBIDDEN`.
4. The primary diagnostic span for `INHERITANCE_FORBIDDEN` SHOULD cover the inheritance clause, and related/help text SHOULD point users toward trait composition.
5. Property declarations use dedicated `prop` / `prop mut` member syntax and do not participate in struct-literal initialization.
6. Struct values are constructed with struct literals: `TypeName{ field_a: expr_a, field_b: expr_b }`. Generic structs use the same constructor head without source type arguments; an expected owner type or stored-field constraints determine the instantiation under `type-system.md`.
7. A struct literal key matching a declared property name MUST fail with `STRUCT_INIT_PROPERTY_FORBIDDEN`.
8. A struct literal key that matches neither a declared stored field nor a declared property MUST fail with `STRUCT_INIT_UNKNOWN_FIELD`.
9. A struct literal MUST NOT initialize the same stored field more than once; duplicates MUST fail with `STRUCT_INIT_DUPLICATE_FIELD`.
10. A struct literal MUST initialize every declared stored field exactly once; missing stored fields MUST fail with `STRUCT_INIT_MISSING_FIELD`.
11. For structs with zero declared stored fields, only `TypeName{}` is valid. A generic zero-field struct requires an independently established expected owner type because the literal supplies no inference constraint.
12. Field initializer expressions are evaluated left-to-right.
13. No implicit per-field default or zero-value initialization is performed in v0.1.
14. `field` members are single-assignment in construction and MUST NOT be reassigned later.
15. `field mut` members are writable through ordinary field assignment after construction.
16. v0.1 has no constructor-specific privileged initialization phase; methods or helper functions do not gain exceptions to Rules 10 and 14.
17. Code in a struct's defining module MAY construct the struct regardless of stored-field visibility.
18. Code outside the defining module MAY use a struct literal only when the struct and every declared stored field are externally accessible under `modules-resolution.md`.
19. If Rule 18 fails because any required stored field is private, construction MUST fail with `VISIBILITY_VIOLATION` at the constructor head before missing-, unknown-, duplicate-, or property-field validation. A defining-module factory is the v0.1 construction surface for a public struct with private storage.

### Assignment Value Compatibility

1. After target validity and writability are established, an assignment RHS
   MUST be assignable to the target's stored type
   (`ASSIGNMENT_TYPE_MISMATCH` otherwise). This includes mutable locals, stored
   fields, and writable explicit dereferences.
2. `ASSIGN_TARGET_NOT_ASSIGNABLE`, `PROPERTY_ASSIGN_FORBIDDEN`, and
   `IMMUTABLE_BINDING_ASSIGN` take precedence over RHS compatibility for the
   same assignment. A property setter call uses ordinary call-argument typing.
3. Errors needed to type the RHS retain their own codes; a missing or poisoned
   RHS type MUST NOT produce a derivative `ASSIGNMENT_TYPE_MISMATCH`.
4. Value compatibility is checked before downstream effect and ownership
   validation of that assignment, including ownership-root replacement.
5. `_ = expr` is a discard, not an assignment to typed storage, and has no
   expected target type. Its discard and ownership rules remain applicable.

### Property Declarations and Accessors

Properties are explicit member declarations backed by ordinary inherent methods.
The accessor methods are declared in source and reserve their callable names in the owning struct's inherent method namespace.
They are not hidden compiler-generated hooks.

Rules:

1. A property declaration has one of the following forms:
   1. `prop name: T { ... }`
   2. `prop mut name: T { ... }`
2. Within one struct body, member names share one namespace across `field`, `field mut`, `prop`, and `prop mut` declarations.
3. Duplicate member names in that shared member namespace MUST fail with `NAME_AMBIGUOUS`.
4. `prop name: T` MUST declare exactly one getter accessor method with signature:
   1. `fn get_name(self: &Self) -> T`.
5. `prop mut name: T` MUST declare exactly two accessor methods with signatures:
   1. `fn get_name(self: &Self) -> T`
   2. `fn set_name!(self: &mut Self, value: T) -> Unit`.
6. Missing required property accessors MUST fail with `PROP_ACCESSOR_MISSING`.
7. Property accessor declarations whose names or signatures do not exactly match the required property contract MUST fail with `PROP_ACCESSOR_SIGNATURE_MISMATCH`.
8. Property accessor contract validation is declaration-level validation and MUST take primary precedence over ordinary receiver/effect-signature checks for the same accessor declaration.
9. Property accessor methods are ordinary inherent methods and MAY be called directly through normal method syntax.
10. For a declared property `name`, the required accessor callable names (`get_name`, and `set_name!` for `prop mut`) are reserved in the owning struct's inherent method namespace.
11. Any additional inherent method declaration using one of those reserved callable names MUST fail with `NAME_AMBIGUOUS`.
12. Getter accessors are non-bang by construction and therefore part of ordinary pure member-read surface.
13. Setter accessors are bang methods by construction and therefore remain explicit effect surfaces.
14. The `prop` / `prop mut` declaration is the sole visibility authority for its required accessor methods. Each accessor inherits the property's visibility and MUST NOT declare an independent `pub` modifier.
15. Property-read lowering and a direct call to the required getter MUST pass the same visibility gate.
16. For `prop mut`, a direct call to the required setter MUST pass that same property visibility gate.

### Field and Property Access

Stored fields and properties use distinct read/write surfaces in v0.1.
The same read syntax can name either a stored field or a property getter, but write syntax stays separate.

Rules:

1. Member read form `recv.name` resolves to one of:
   1. raw stored-field read when `name` is a declared `field` / `field mut`,
   2. property-read lowering `recv.get_name()` when `name` is a declared `prop` / `prop mut`.
2. Stored-field assignment form `recv.name = value` is valid only for declared `field mut` members.
3. Assignment to a declared immutable `field` member MUST fail with `IMMUTABLE_BINDING_ASSIGN`.
4. Assignment to a declared property name (`recv.name = value`) MUST fail with `PROPERTY_ASSIGN_FORBIDDEN`, even for `prop mut`.
5. Writable property update uses the explicit setter call surface `recv.set_name!(value)`.
6. Property-read lowering and explicit setter-call surface participate in ordinary inherent method lookup and effect checking.

Example:

```eglur
c.count          // stored-field read, or property read lowered to c.get_count()
c.count = 1      // valid only when count is a declared field mut
c.set_count!(1)  // writable property update for prop mut count
```
