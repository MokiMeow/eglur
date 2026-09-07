# Standard Library Catalog Signature Metalanguage

Scope: Normative for v0.1 unless a catalog declaration or section states otherwise.

## Status and Scope

Fenced blocks labeled `eglur-catalog` describe compiler-provided standard-library API declarations.
They are normative interface records, not Eglur source text.

Rules:

1. An implementation MUST provide every required declaration in these blocks with the specified callable identity, parameter contract, result contract, constraints, defaults, and effect marker.
2. The compiler MUST NOT accept catalog-only syntax in `.eglur` source files merely because that syntax appears in a catalog block.
3. Plain `text`, `eglur`, and other fenced blocks are examples, source forms, pattern grammars, or implementation vocabularies; they are not catalog declarations unless the surrounding specification explicitly says otherwise.
4. Module prose MAY refine the behavior of a catalog declaration but MUST NOT introduce a new declaration form, callback kind, pack kind, or implicit type-variable rule.
5. A required API that cannot be represented by this file's declaration model MUST first extend this file normatively. Per-module ad hoc signature notation is forbidden.
6. Implementations MAY encode the catalog in compiler tables, generated source, or another internal representation. Conformance depends on the declared semantics, not on parsing the Markdown blocks.
7. Whitespace and line breaks MAY separate grammar tokens. A declaration MAY span lines but MUST remain one complete grammar production.
8. A language-intrinsic type already defined by the language specification MAY be re-exported or enumerated by the prelude without a duplicate catalog type declaration. Catalog declarations MAY use that type as an owner or component type. Stdlib-owned types that are not language intrinsics MUST be declared in a catalog block.

## Module Binding

Every catalog declaration belongs to exactly one fully qualified standard-library module:

1. The default module is the single backticked `std::...` path in the document's level-1 heading.
2. The nearest enclosing heading that contains exactly one backticked `std::...` path overrides the document default for catalog blocks in that section.
3. A catalog block is invalid when neither rule selects one module, or when the selected heading names more than one `std::...` path.
4. Unqualified declared names are relative to the selected module. `reexport` targets are absolute catalog paths.
5. One catalog block MUST NOT mix declarations from multiple modules.
6. The fully qualified module path is part of declaration identity. Repeating an identical declaration in two blocks for the same module is invalid; cross-module callables with the same unqualified name are distinct.

## Declaration Grammar

The grammar below is descriptive EBNF. `Identifier`, `BangIdent`, literals, qualified paths, references, tuples, and ordinary type application reuse their spellings from the language grammar where applicable.

```ebnf
CatalogBlock       ::= CatalogDecl*
CatalogDecl        ::= Availability? (
                         TypeDecl
                       | ReexportDecl
                       | TraitDecl
                       | ImplDecl
                       | ValueDecl
                       | FnDecl
                       | CallableValueDecl
                       | SchemaDecl
                       | TransformDecl
                     )

Availability       ::= "optional"

TypeDecl           ::= "opaque" "type" TypeHead
                     | "alias" "type" TypeHead "=" CatalogType
                     | "sum" "type" TypeHead "=" Variant ("|" Variant)*
                     | "union" "type" TypeHead "=" CatalogType ("|" CatalogType)*
                     | "meta" "type" TypeHead

TypeHead           ::= Identifier TypeGenericParams?
TypeGenericParams  ::= "[" CatalogGenericParam ("," CatalogGenericParam)* "]"
GenericParams      ::= "[" Identifier ("," Identifier)* "]"

Variant            ::= Identifier
                     | Identifier "(" VariantTupleFields? ")"
                     | Identifier "{" VariantRecordFields? "}"
VariantTupleFields ::= VariantTupleField ("," VariantTupleField)* ","?
VariantTupleField  ::= CatalogType DefaultValue?
VariantRecordFields ::= VariantRecordField ("," VariantRecordField)* ","?
VariantRecordField ::= Identifier ":" CatalogType DefaultValue?

ReexportDecl       ::= "reexport" "type" Identifier "=" CatalogPath
                     | "reexport" "trait" Identifier "=" CatalogPath
                     | "reexport" "fn" FnName "=" CatalogPath
CatalogPath        ::= Identifier ("::" Identifier)+

TraitDecl          ::= "closed"? "trait" Identifier GenericParams?
                       "{" TraitMember* "}"
TraitMember        ::= AssocTypeDecl | TraitFnDecl
AssocTypeDecl      ::= "type" Identifier
TraitFnDecl        ::= "overload"? "fn" FnName CatalogArgs?
                       "(" ParamList? ")" "->" CatalogType

ImplDecl           ::= "impl" GenericParams? TraitRef "for" SourceType
                       "{" AssocTypeBinding* "}"
TraitRef           ::= SourceType
AssocTypeBinding   ::= "type" Identifier "=" SourceType

ValueDecl          ::= "value" Identifier ":" CatalogType

FnDecl             ::= "overload"? "fn" CallableHead CatalogArgs?
                       "(" ParamList? ")" "->" CatalogType RequiresClause?
CallableHead       ::= FnName | Owner "." FnName
FnName             ::= Identifier | BangIdent
Owner              ::= SourceType

CallableValueDecl  ::= "callable" Identifier CatalogArgs?
                       "(" ParamList? ")" "->" CatalogType RequiresClause?
SchemaDecl         ::= "schema" Identifier "{" MetaPattern "}" "->" CatalogType
TransformDecl      ::= "transform" FnName CatalogArgs?
                       "(" ParamList? ")" "->" CallbackFamily

CatalogArgs        ::= "[" CatalogArg ("," CatalogArg)* "]"
CatalogArg         ::= CatalogGenericParam | CatalogType
CatalogGenericParam ::= Identifier (":" TraitBoundList)?

ParamList          ::= Param ("," Param)* ","?
Param              ::= "named"? Identifier ":" ParamType DefaultValue?
                     | PackParam
ParamType          ::= CatalogType | CallbackType | CallbackFamily
PackParam          ::= Identifier ":" "named"? CatalogType "..."
DefaultValue       ::= "=" CatalogConstExpr

CallbackType       ::= "callback" "[" CallbackKind "]"
                       "(" CatalogTypeList? ")" "->" CatalogType
CallbackFamily     ::= "callback" "[" ("pure" | "effectful") "]"
CallbackKind       ::= "pure"
                     | "effectful"
                     | "scoped-resource"
                     | "task-thunk"

RequiresClause     ::= "requires" Requirement ("+" Requirement)*
                       ("when" CatalogCondition)?
Requirement        ::= Identifier "[" CatalogTypeList? "]"

CatalogTypeList    ::= CatalogType ("," CatalogType)*
CatalogType        ::= SourceType
                     | CallbackType
                     | CatalogTuple
                     | TuplePack
                     | CatalogType "|" CatalogType
CatalogTuple       ::= "(" CatalogType "," CatalogType
                       ("," CatalogType)* ")"
TuplePack          ::= "(" CatalogType "..." ")"

SourceType         ::= TypeExpr

MetaPattern        ::= "<operands>" "->" "<outputs>"
                     | "<operand>" "->" "<output>"
                     | "<operands>" "->" "<out_axes>"

CatalogConstExpr   ::= CatalogSignedNumber
                     | StrLiteral
                     | BoolLiteral
                     | AxisNameLit
                     | AxisIndexLit
                     | CatalogValuePath
                     | CatalogValuePath "(" CatalogConstList? ")"
CatalogSignedNumber ::= "-"? (IntLiteral | FloatLiteral)
CatalogValuePath   ::= Identifier ("::" Identifier)*
CatalogConstList   ::= CatalogConstExpr ("," CatalogConstExpr)* ","?

CatalogCondition   ::= CatalogCondOr
CatalogCondOr      ::= CatalogCondAnd ("||" CatalogCondAnd)*
CatalogCondAnd     ::= CatalogCondNot ("&&" CatalogCondNot)*
CatalogCondNot     ::= "!" CatalogCondNot | CatalogCondAtom
CatalogCondAtom    ::= CatalogConditionValue
                     | CatalogConditionValue CatalogComparator CatalogConditionValue
                     | "(" CatalogCondition ")"
CatalogConditionValue ::= CatalogConstExpr | ReceiverObservation
ReceiverObservation ::= "self" "." Identifier "(" ")"
CatalogComparator  ::= "==" | "!=" | "<" | "<=" | ">" | ">="
```

`TypeExpr`, `TraitBoundList`, `Identifier`, `IntLiteral`, `FloatLiteral`,
`StrLiteral`, `BoolLiteral`, `AxisNameLit`, and `AxisIndexLit` are imported by
exact production reference from the normative grammar in
`../language/syntax.md`. Catalog lexing uses those productions unchanged.

Additional closure rules:

1. A `SourceType` MUST parse as that imported `TypeExpr`. Language-owned types retain their ordinary v0.1 semantic restrictions; catalog-owned opaque, alias, sum, union, and meta names resolve only through declarations in the selected module or an explicit catalog path.
2. A `MetaPattern` is one of the three literal productions above. The schema declaration's module rules MUST map each placeholder to a named source grammar production; angle-bracket placeholders are not open text substitution.
3. A `CatalogConstExpr` admits no operators, arbitrary calls, field access, blocks, collections, or user code. A call form MUST resolve to a pure total catalog constructor named by that module's adjacent rules.
4. A `CatalogCondition` MUST type-check to `Bool`. A bare identifier MUST name a declaration parameter or a catalog constant. A receiver observation MUST name a pure zero-argument method on the declared receiver. No other call or member form is valid.

## Type Declaration Kinds

Catalog type declarations are explicit about identity and representation:

1. `opaque type T` declares a nominal API type whose representation and constructors are hidden unless separately exposed.
2. `alias type T = U` declares a transparent catalog spelling for `U`. It does not imply that users can write the same declaration as a source `type` alias.
3. `sum type T = A | B(...) | C{...}` declares a closed constructor set owned
   by `T`. Parenthesized payloads are positional; braced payloads are named.
   Catalog declarations spell the owned variants unqualified, but source
   expressions and patterns use `T::A`, `T::B(...)`, and `T::C { ... }`.
   Variants are not standalone source import symbols. Defaults are valid only
   on a trailing payload suffix.
4. `union type T = A | B` declares an accepted family of existing types and introduces no constructors or runtime union representation by itself.
5. `meta type T` declares a compiler-known type relation, schema family, argument family, or other catalog-only type-level name. A meta type is not user-definable source syntax and has no runtime representation unless its module rules explicitly provide one.
6. `reexport type T = path::T` preserves the identity of the target type. It MUST NOT create a second nominal type.
7. A bound parameter in a type head, such as `K: Eq + Hash`, is a well-formedness requirement on every instantiation of that catalog type. All declarations whose owner or signature contains that instantiation inherit the requirement; it is not an operation-local `requires` clause.
8. A type declaration without one of these six forms is invalid in an `eglur-catalog` block.
9. Every baseline `meta type` MUST have adjacent module rules that close its accepted members or transformation and, when values cross the source boundary, its source-visible representation or normalization. `meta type` MUST NOT hide an unresolved baseline signature. Non-baseline declarations MAY leave explicitly listed design choices unresolved.

Catalog unions MAY appear outside the source language's permitted `Res` error-union position. Such a union describes accepted API alternatives and MUST be implemented as a closed builtin family or an equivalent overload set; it does not enable general source union types.

## Traits, Implementations, and Trait Re-exports

Catalog trait and impl declarations are source-shaped interface records. They do
not contain executable source bodies.

Rules:

1. A catalog `trait` declares one public source-visible trait in the selected
   module. `closed trait` has the impl-authority semantics from
   `../language/traits.md`; it does not enable exhaustive matching, negative
   reasoning, specialization, dynamic dispatch, or runtime introspection.
2. A catalog associated type has no default, bound, generic parameters, or
   initializer. Its identity is the declaring trait plus its name.
3. A nested catalog trait method is required and has no default body. Its
   canonical callable identity is `module::Trait.method`; method syntax and
   fully qualified calls or named callable references dispatch to that identity
   under the ordinary trait rules.
4. `Self` in a catalog trait method denotes the implementing source type.
   `Self::Name` denotes an associated type declared by that trait.
5. A catalog `impl` asserts a required standard-library conformance. Its generic
   parameters bind across the trait reference, target type, and associated-type
   bindings and MUST satisfy the ordinary impl-parameter, coherence, associated
   type, and visibility rules. Its `TraitRef` MUST resolve to a trait.
6. A catalog impl MUST bind every associated type declared by its trait exactly
   once and MUST bind no unknown associated type.
7. Catalog impl records omit method bodies and method declarations. An
   implementation MUST nevertheless supply behavior matching every required
   trait method. The impl record creates no impl-owned or concrete-owner
   callable identity.
8. A catalog impl declaration does not by itself forbid additional conforming
   impls from the declaring package. A module that requires an exact public
   implementor set MUST state that closed set adjacent to its declarations.
9. `reexport trait Local = path::Trait` preserves the target trait, associated
   type, method, and impl identities. It adds one visible path and MUST NOT
   create a second trait or duplicate method candidate.
10. Catalog traits and impl records are static interface metadata. They do not
    introduce trait values, impl values, dynamic dispatch, or runtime
    reflection.
11. A method owned by a catalog-declared trait MUST appear inside that
    `TraitDecl` and MUST NOT be restated as a top-level owner-qualified `FnDecl`.
    Top-level owner-qualified trait methods remain valid only for language or
    prelude traits whose trait declarations are intrinsic rather than catalog
    declarations.

## Type Variables and Specialization

Catalog declarations do not infer type variables from parameter or result positions.

1. Generic parameters in a type or trait head bind within that declaration.
2. Variables in a callable owner, such as `T` in `Seq[T].map`, bind according to the declared generic slots of the owner type.
3. Additional callable variables MUST appear in the callable's `CatalogArgs`,
   such as `U` in `Seq[T].map[U]`. A binder scopes over the complete
   declaration, including a generic owner such as `T` in `T.tap[T]`.
   `C: ReadableTensor + AffineTensor` records the same explicit named generic
   parameter and nominal bounds as the corresponding source
   `GenericParam`; it is not catalog-only requirement unification.
4. A free function with generic variables MUST bind them all in `CatalogArgs`.
5. A resolved concrete type in `CatalogArgs` fixes a specialization rather than introducing a variable. Thus `nan[f32]` is a fixed `f32` specialization while `zeros[T]` binds `T`.
6. A type variable that first appears only in a parameter, callback, requirement, default, or result is invalid.
7. Generic parameters in a catalog impl bind across its complete impl record.
   Every such parameter MUST constrain the trait reference or target type under
   the source impl rules.
8. Catalog binders are declaration notation. They do not define source call-site generic syntax. Source calls continue to follow the language grammar, except where a module explicitly defines a dedicated surface such as tensor constructor dtype brackets.
9. A bound catalog generic parameter MAY be used as the base of an associated
   type projection under the source projection rules, for example
   `C::Elem`. Catalog declarations MUST NOT use this notation to add
   associated-type equality constraints or reverse projection inference.

Unconstrained catalog variables range over ownership-free, reference-free source types under `language/calls-references.md`. Module rules or a `requires` clause MAY narrow that domain. Catalog callback and receiver signatures may still write explicit references outside those variables.

## Functions, Values, Owners, and Receivers

`fn` declares a compiler-provided callable. It covers free functions, owner-qualified methods, and owner-qualified associated functions.

1. `Owner.fn(...)` with no `self` parameter is an associated function supplied by the catalog. It does not make associated/static methods available to user declarations.
2. An owner-qualified method MUST place `self` first.
3. A receiver type MUST be `Self`, `&Self`, `&mut Self`, or the equivalent explicit owner type used by older catalog entries.
4. `Self` refers to the catalog owner, including a concrete owner such as `Own[ByteReader]`.
5. A trailing `!` has the same effect visibility meaning as a source bang callable.
6. `reexport fn local = path::target` preserves the complete target signature and callable identity except for its visible path. A re-export MUST NOT restate a divergent local signature.
7. `value name: T` declares an exported, immutable catalog value. Its enclosing module rules MUST define the value; this form does not permit arbitrary catalog initializers.
8. An owner MAY resolve to a language or catalog type, a trait with a source-callable method such as `Clone`, a catalog meta type, or a generic owner variable bound in `CatalogArgs`. These categories do not become interchangeable in source type positions.

## Callback Contracts

Callback contracts are explicit because the source language has only the pure `Fn(...) -> R` callable type in v0.1.

1. `callback[pure](P...) -> R` is equivalent to the source callable expectation `Fn(P...) -> R` and rejects effectful callback bodies.
2. `callback[effectful](P...) -> R` is a direct API-level contract that permits bang calls. It is not a first-class source `Fn!` type.
3. `callback[scoped-resource](&mut Own[R]) -> Res[T, E]` uses the exact-once, non-escaping resource rules in `conventions.md`.
4. `callback[task-thunk]() -> T` uses the child-task body and capture rules in
   `../language/concurrency.md`.
   It accepts either an inline zero-parameter closure under the API's task-body
   effect authorization or a preconstructed pure source `Fn() -> T` value.
   Accepting the latter preserves its existing closure environment and does not
   synthesize a first-class effectful callable type.
5. Bare arrow notation such as `T -> U` or `() -> T` is invalid as a parameter type in an `eglur-catalog` block.
6. `callback[pure]` and `callback[effectful]` without a parameter/result list are callable-family markers. They are valid only in a `transform` declaration whose adjacent normative rules provide a total input-to-output callable mapping.
7. Callback effect authorization does not relax source closure capture rules:
   1. ordinary callback literals follow `../language/calls-references.md`,
   2. task thunks additionally follow `../language/concurrency.md`,
   3. a `callback[scoped-resource]` parameter is supplied by the API and is not
      a captured local.
8. Callback-taking catalog callables participate in compile-time effect
   summaries as defined by `../language/effects.md`:
   1. the callable's normative API semantics determine its own summary, and
      the catalog implementation MUST encode that summary as trusted metadata,
   2. `callback[pure]` contributes no additional effect,
   3. `callback[effectful]`, `callback[scoped-resource]`, and
      `callback[task-thunk]` positions propagate the actual argument summary,
   4. catalog metadata MUST identify every propagating callback position
      deterministically,
   5. aliases and re-exports preserve both the own summary and callback
      propagation positions.

The normative trusted metadata is authored in `effect-summaries.yaml`. Callable
identity and callback spelling remain owned by the `eglur-catalog` declarations;
the effect-catalog validator MUST reject any missing, extra, or divergent
machine record. Reference tables are generated with
`pixi -q run --locked render-effects` and MUST NOT be
maintained independently.

## Defaults and Packs

Default arguments and variadic packs are catalog capabilities, not source declaration syntax.

1. A default is valid only on a trailing suffix of ordinary parameters before any pack.
2. `CatalogConstExpr` is restricted to literals, built-in residual `None`,
   owner-qualified nullary sum variants, qualified constants, and pure total
   constructor calls whose arguments are themselves catalog constants.
3. Overload applicability permits an omitted trailing argument only when that parameter has a default. Defaults are inserted after a unique overload is selected and before invocation.
4. `items: T...` is a positional homogeneous pack of zero or more `T` values.
5. `items: named T...` is a named-argument pack. Every captured argument is checked independently against the catalog family `T`.
6. `named p: T` is a named-only ordinary parameter. A call MUST spell the parameter name when supplying it.
7. A parameter list MAY contain at most one pack, and the pack MUST be last.
8. `(A, B)` is an ordinary catalog tuple. Its elements MAY include catalog-only callback contracts.
9. `(T...)` is a tuple result pack whose exact arity is fixed by the associated schema or transform rules.
10. Bare `...`, numbered pseudo-parameters such as `t1, ..., tN`, and untyped named-argument packs are invalid in an `eglur-catalog` block.

## Requirements and Overloads

`requires` records catalog applicability constraints that are broader than source nominal trait bounds.

1. Every requirement is checked after generic instantiation and before the call is accepted.
2. `A + B` requires all listed predicates.
3. For `requires P[T] when condition`, the requirement is waived only when `condition` is statically false. If it is true or not statically decidable, `P[T]` is required.
4. `CatalogCondition` is a pure boolean expression over parameters, catalog constants, and receiver observations named by the declaration's module rules. It MUST NOT perform effects.
5. Comments are not part of the catalog grammar. Explanations, requirements, availability notes, overload notes, and other prose MUST appear outside an `eglur-catalog` block unless encoded by a declaration production.
6. Every member of a same-owner, same-name overload set MUST use the `overload` prefix. A repeated callable name without that marker is invalid.
7. `optional` applies to one declaration. Non-baseline status inherited from an enclosing section does not need to be repeated per declaration.
8. Catalog `requires` MAY express conditional, value, dtype, shape, device, or compiler-known predicates and MUST NOT be interpreted as source `GenericParam` trait-bound syntax.

## Schema Values and Callable Transforms

Tensor and similar compiler-known surfaces use three additional closed declaration kinds:

1. `schema name{ pattern } -> SchemaType` declares a compiler-recognized expression that produces a schema value. `pattern` MUST be governed by a normative grammar in the same module.
2. `callable SchemaType[Args](...) -> R` declares how values of a schema family are called. Pack arity and result-pack arity MUST be fixed by that schema's normative rules.
3. `transform name(...) -> callback[kind]` declares a callable-to-callable adapter such as `map_axis` or `grad`. The same section MUST exhaustively define accepted input callable families, output callable parameters, and output result types. An unlisted callable family MUST fail with `CALL_ARG_TYPE_MISMATCH` unless that section assigns a more specific diagnostic.
4. Schema and transform declarations do not introduce user-definable higher-kinded types, dependent types, or arbitrary callable reflection.
5. A compiler-known schema adapter MAY receive invocation-derived expected
   parameter constraints for unannotated inline closure literals only when
   this section lists the exact adapter as an expression-local specialization.
   The adapter's module MUST define:
   1. the complete expression shape that supplies the invocation input,
   2. the finite typing order from that input through the callback chain,
   3. the desugared call forms that count as the same expression shape,
   4. the diagnostic when construction-site constraints remain incomplete.
6. The only expression-local schema specialization in v0.1 is
   `std::tensor::xform::ReduceSchema.reduce_by`. It does not permit constraints
   to cross a binding initializer, general backward inference, polymorphic
   local schema values, delayed specialization of
   named/generic/overloaded/placeholder/partial-application callable forms, or
   later-use specialization of a `transform` result such as `map_axis`.

## Canonical Examples

```text
opaque type Path
opaque type Map[K: Eq + Hash, V]
sum type TextError = Io(IoError) | InvalidUtf8
sum type LinkMode = Follow | Reject
union type AxisSel = AxisIndex | AxisName
alias type NameOverlay = Map[AxisIndex, AxisName]
meta type Promote[A, B]
reexport type IndexError = std::prelude::IndexError

closed trait Readable {
  type Elem
  fn read(self: &Self) -> Self::Elem
}
impl[T] Readable for Seq[T] {
  type Elem = T
}

fn Path.from_str(s: Str) -> Path
fn readable_identity[C: Readable](x: C) -> C
fn Seq[T].map[U](self: Self, f: callback[pure](T) -> U) -> Seq[U]
fn Seq[T].group_by[K](self: Self, key: callback[pure](T) -> K) -> Map[K, Seq[T]] requires Eq[K] + Hash[K]
fn with_reader![T](path: Path, links: LinkMode, f: callback[scoped-resource](&mut Own[ByteReader]) -> Res[T, IoError]) -> Res[T, ScopedFailure[IoError]]
fn format(template: Str, args: named FormatArg...) -> Res[Str, FormatError]
```

These examples illustrate the catalog metalanguage only. They do not add `opaque`, `sum`, `union`, `meta`, catalog defaults, packs, requirements, or catalog callback kinds to Eglur source syntax.
