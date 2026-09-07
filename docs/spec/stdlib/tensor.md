# `std::tensor` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Core Types

```eglur-catalog
opaque type AxisIndex
opaque type AxisName
union type AxisSel = AxisIndex | AxisName
alias type NameOverlay = Map[AxisIndex, AxisName]
opaque type Tensor[T]
opaque type TensorView[T]
meta type AnyTensor
meta type RangeSpec

opaque type AxisError
opaque type ShapeError
reexport type IndexError = std::prelude::IndexError
opaque type ContractError
opaque type DTypeError
reexport type NumericError = std::prelude::NumericError
opaque type CastError
opaque type DeviceError
opaque type DeterminismError
opaque type AmbiguousDims
opaque type InconsistentDims
opaque type NotAView
opaque type MultiInputNotAllowed
opaque type NumelMismatchGrow
opaque type NumelMismatchShrink
opaque type SegmentStraddle
opaque type TransformError
opaque type EinopError
opaque type MapAxisError
opaque type GradError
opaque type DType[T]
opaque type Device
sum type DeterminismProfile = Default | Strict

value bool: DType[Bool]
value i8: DType[i8]
value i16: DType[i16]
value i32: DType[i32]
value i64: DType[i64]
value u8: DType[u8]
value u16: DType[u16]
value u32: DType[u32]
value u64: DType[u64]
value f16: DType[f16]
value bf16: DType[bf16]
value f32: DType[f32]
value f64: DType[f64]
value complex64: DType[complex64]
value complex128: DType[complex128]

meta type InAxis
meta type Promote[A, B]
meta type Schema
meta type ViewSchema
meta type ViewResult[T]
meta type RearrangeSchema
meta type RearrangeResult
meta type InflateSchema
meta type ReduceSchema
meta type ReduceBySchema
meta type ReduceByArg
meta type ReduceByResult
meta type ReduceByErrors
meta type ReduceAxisGroup
meta type OpSchemaN
meta type ContractResult
meta type EinopSchemaN
meta type EinopResult
```

`AxisIndex` and `AxisName` MUST be distinct types.

`IndexError` and `NumericError` MUST re-export the corresponding shared `std::prelude` nominal types; implementations MUST NOT define tensor-local types with those names.

`AxisIndex` has lexical form `#<nat>`, and `AxisName` has lexical form `'<ident>`.
`#i` addresses positional axes, while `AxisName` provides semantic overlay metadata.

Literal construction identity is language-owned:

1. `#i` always constructs this section's canonical `AxisIndex`, and `'name`
   always constructs this section's canonical `AxisName`.
2. Literal construction requires no `std::tensor` import and does not perform
   ordinary type/value lookup.
3. Import aliases, same-spelled local or module declarations, and expected
   types MUST NOT change either literal's canonical result type.
4. User-defined types cannot opt into these literal forms in v0.1.

`AxisIndex` and `AxisName` implement `Eq` and `Hash`. `AxisIndex` equality and
hashing use the represented non-negative positional index. `AxisName` equality
and hashing use the exact case-sensitive name. Both hash implementations pass
their represented value through the required seeded `Hash` contract.
`Tensor[T]` and `TensorView[T]` implement neither `Eq` nor `Hash` in v0.1;
tensor elementwise or whole-value equality is not provided by the scalar
`==`/`!=` operator.

Detailed tensor propagation rules are normatively defined in [the axes specification](../axes.md).

`DType[T]` coverage MUST include at least:

1. `bool`
2. Signed integers: `i8`, `i16`, `i32`, `i64`
3. Unsigned integers: `u8`, `u16`, `u32`, `u64`
4. Floating types: `f16`, `bf16`, `f32`, `f64`
5. Complex types: `complex64`, `complex128`

The catalog `value` declarations above are typed source expression witnesses for those dtypes. The `bool` token has type `DType[Bool]`; each numeric token named `T` has type `DType[T]`. The tokens inhabit the value namespace and do not replace or alias same-spelled type-namespace entries.

Descriptor values follow ordinary value-name resolution:

1. A source expression that uses a descriptor MUST import it explicitly from
   `std::tensor`, for example:

   ```eglur
   import std::tensor::{f64, ones}
   import std::tensor::prim::astype

   let y = astype(ones[f32](4), f64)?
   ```

2. Descriptor values are not prelude bindings and are not supplied by expected
   `DType[T]` context. An unqualified descriptor use with no visible value
   binding fails with `NAME_NOT_FOUND`.
3. Importing a descriptor affects only the value namespace. The same-spelled
   prelude scalar type remains available in type position.
4. A dtype argument inside a dedicated constructor bracket is not a
   descriptor-value expression and does not require the corresponding value
   import.

`Tri` does not inhabit the v0.1 `DType[T]` family.
It is therefore outside the required `cast-matrix.md` axes, and neither
`Tensor[Tri]` nor tensor `astype(..., Tri)` is part of baseline conformance.

Implementations MAY provide additional numeric dtypes (for example float8 variants) but MUST document promotion and kernel support behavior.
Each additional dtype `T` MUST expose a same-spelled `DType[T]` descriptor value in the value namespace.

Every `DType[T]` MUST implement `std::prelude::Display`. The callable identity selected by `dtype.to_str()` is `std::prelude::Display.to_str`; the generic prelude `to_str(dtype)` adapter dispatches to that same implementation. Both forms MUST return the canonical lowercase dtype token used in this section (for example `u8`, `bf16`, `complex64`).
`std::tensor` MUST NOT declare a distinct inherent `DType[T].to_str` method or a tensor-local free `to_str` overload. This keeps formatting lookup on the prelude `Display` contract without creating a tensor-specific display callable identity.
Additional implementation dtypes MUST implement `Display` and document their canonical `DType[T]` formatting token.

The block above uses the normative [catalog signature metalanguage](catalog-signatures.md), not Eglur source syntax.
`AnyTensor` is the closed catalog argument family for explicit readable tensor
carriers. After a schema fixes invocation arity, each `AnyTensor` parameter
occurrence expands to a fresh named source generic `Ci: ReadableTensor` and a
source parameter of type `Ci`; an `AnyTensor...` pack expands positionally to
`C0, ..., Cn` under those bounds. The corresponding element dtype is
`Ci::Elem`. This normalization preserves each concrete `Tensor[Ti]` or
`TensorView[Ti]` carrier and introduces no source union, erased runtime value,
implicit conversion, or reverse associated-type inference. `AnyTensor` MUST
NOT appear in source type positions.
`DType[T]` is an arity-one typed witness family. Bare `DType` is not a runtime-erased descriptor type, and v0.1 provides no implicit common type for descriptor values with different `T` arguments.
`RangeSpec` is the compiler-known argument family containing only
`IndexItem::All`, `IndexItem::Slice(...)`, and `IndexItem::Rest` values.
`InAxis` is the compiler-known argument family containing `AxisSel` values and `None`.
`Promote[A, B]` is the closed numeric dtype relation defined by `conventions.md`.

`DeterminismProfile` is the typed call-site surface for per-operation tensor determinism requests.
It has exactly two v0.1 baseline values: `DeterminismProfile::Default` and `DeterminismProfile::Strict`.
String profile names such as `"default"` or `"strict"` are not accepted as `DeterminismProfile` values.
Diagnostic `data.profile` values use canonical lowercase tokens (`default`, `strict`) regardless of source spelling.

Axis helper APIs:

```eglur-catalog
fn axis(i: Nat) -> AxisIndex
fn AxisIndex.to_nat(self: AxisIndex) -> Nat
```

Rules:

1. `axis(i)` and `#i` construct non-negative positional axis selectors.
2. Signed runtime positions MUST be converted through `Nat.from_i64` before construction; failed conversion uses `DOMAIN_INVALID_VALUE`.


### Tensor Carrier Capabilities and Introspection

```eglur-catalog
closed trait ReadableTensor {
  type Elem
  fn rank(self: &Self) -> Nat
  fn shape(self: &Self) -> Vec[Nat]
  fn dtype(self: &Self) -> DType[Self::Elem]
  fn device(self: &Self) -> Device
  fn names(self: &Self) -> NameOverlay
  fn item(self: &Self) -> Res[Self::Elem, IndexError]
  fn axis_of(self: &Self, name: AxisName) -> Res[AxisIndex, AxisError]
}

closed trait AffineTensor {}

impl[T] ReadableTensor for Tensor[T] {
  type Elem = T
}
impl[T] ReadableTensor for TensorView[T] {
  type Elem = T
}
impl[T] AffineTensor for Tensor[T] {}
impl[T] AffineTensor for TensorView[T] {}
```

Rules:

1. The complete public v0.1 implementor set of both `ReadableTensor` and
   `AffineTensor` is exactly `Tensor[T]` and `TensorView[T]`.
2. Both required `ReadableTensor` impls bind `Elem = T`. An implementation with
   a different public binding is non-conforming.
3. Implementations MAY use additional private internal carriers only when those
   carriers never appear in public signatures, source-nameable conformance, or
   required conformance behavior. An additional public carrier impl is
   non-conforming.
4. A package other than the standard-library package MUST NOT implement either
   closed trait (`CLOSED_TRAIT_IMPL_FORBIDDEN`).
5. `ReadableTensor` is re-exported by `std::prelude`; concrete and generic
   `x.rank()`-style calls therefore need no explicit trait import.
6. `AffineTensor` is a methodless marker and is not in the prelude. Source that
   names it in a bound MUST import it explicitly from `std::tensor`.
7. `AffineTensor` marks a carrier whose retained logical snapshot can participate
   in the zero-copy affine view paths defined by `../language/tensor-model.md`.
   It does not assert that every transform is affine or that the carrier itself
   is a view.
8. The canonical query callable identities are
   `ReadableTensor.rank`, `ReadableTensor.shape`, `ReadableTensor.dtype`,
   `ReadableTensor.device`, `ReadableTensor.names`, `ReadableTensor.item`, and
   `ReadableTensor.axis_of`. `Tensor` and `TensorView` MUST NOT declare distinct
   inherent callables with those names.
9. Fully qualified calls such as `ReadableTensor.rank(&x)` and named callable
   references such as `ReadableTensor.rank` select that same trait method.
   Callable-value coercion MUST use the expected `Fn` type to determine the
   concrete `Self` and then validate the applicable impl.
10. Concrete-owner references such as `Tensor.rank` and `TensorView.rank` do not
    exist. Attempting to select one MUST fail with `METHOD_NOT_FOUND`; the
    available alternatives are method syntax and the trait-owned callable.
11. `item()` MUST succeed only when `rank(self) == 0`.
12. Calling `item()` on rank greater than 0 MUST fail with
    `ITEM_REQUIRES_RANK0`.
13. Implicit `Tensor[T] -> T` or `TensorView[T] -> T` scalar conversion MUST NOT
    be performed by the language.
14. `TensorView` query methods, including `item()` and `axis_of()`, MUST read the
    retained snapshot's logical values and metadata without materializing the
    view.
15. The finite implementor set is a portability and catalog-validation fact
    only. It MUST NOT enable exhaustive matching, closed-world negation,
    specialization, dynamic dispatch, or runtime trait/impl introspection.
16. Because v0.1 exposes no source-level implementor-set introspection,
    conformance with Rules 1-3 is established by auditing the implementation's
    exported catalog inventory, not by a source program enumerating impls.


### TensorView Materialization

```eglur-catalog
fn TensorView[T].materialize(self: &Self) -> Tensor[T]
```

Rules:

1. `materialize()` MUST return an owning tensor with the view's logical element
   values, dtype, device, shape, axis order, and name overlay at call time.
2. The returned tensor MUST be mutation-independent from the view, every copy of the
   view, and the tensor from which the view was created.
3. `materialize()` is an explicit realization boundary. Callers MUST NOT rely on it
   being zero-copy. Implementations MAY reuse storage only when Rule 2 and every
   owning-`Tensor` representation invariant, including the primitive movement
   guarantees below, remain satisfied.
4. Implicit conversion and argument coercion MUST NOT invoke `materialize()`;
   implementations MUST NOT insert this call during type checking or lowering.
5. Retention, copying, and mutation-isolation semantics are defined normatively in
   `../language/tensor-model.md`.


### Public Tensor Carrier Matrix

The following matrix classifies the minimum carrier contract of every public
v0.1 tensor API family. A more specific signature later in this file MAY impose
dtype, shape, arity, error, or concrete-carrier constraints, but it MUST NOT
weaken the carrier boundary recorded here.

| API family | Input or callback boundary | Successful output | Carrier rule |
| --- | --- | --- | --- |
| `axis`, `AxisIndex.to_nat`, dtype values, schema constructors, schema configuration, and linalg schema values | No tensor value | Metadata or schema value | Carrier-independent |
| `rank`, `shape`, `dtype`, `device`, `names`, `item`, and `axis_of` | `ReadableTensor` receiver | Metadata or scalar value | Logical read only; no materialization |
| `TensorView.materialize` | Concrete `TensorView[T]` receiver | `Tensor[T]` | Explicit realization and copy-isolation boundary |
| `with_names`, `rename`, `with_names!`, and `rename!` | Concrete owning `Tensor[T]`; bang forms require `&mut Tensor[T]` | `Tensor[T]` or `Unit` | Owning metadata update; no `TensorView` mutation surface |
| `zeros`, `ones`, `nan`, and `rand!` | No tensor value | `Tensor[T]` | Owning construction |
| `view{...}` invocation | `C: ReadableTensor + AffineTensor` | One or more `TensorView[C::Elem]` values | Zero-copy affine composition; no implicit realization |
| `rearrange{...}`, `inflate{...}`, and default `reduce{...}` invocation | Each tensor operand is an explicit `ReadableTensor` carrier | One or more `Tensor[...]` values | Logical read followed by materialization |
| `reduce_by` invocation and callbacks | Invocation input is `C: ReadableTensor + AffineTensor`; phase `i` receives a read-only `TensorView[T_i]` | Owning `Tensor[T_N]` after the ordered chain `T_0 -> ... -> T_N` | Phase views expose retained logical subtensors; no mutable carrier |
| Unary, binary, conditional, wrapping, cast, and reduction primitives | Every tensor operand is an explicit `ReadableTensor` carrier | `Tensor[...]` | Logical read followed by materialization |
| `permute`, `unsqueeze`, `squeeze`, `expand`, and range-only `slice` | `C: ReadableTensor + AffineTensor` | `TensorView[C::Elem]` | Zero-copy affine composition; no implicit realization |
| `reshape` | Concrete `Tensor[T]` or `TensorView[T]` overload | `TensorView[T]` | Owning input has the total valid-reshape contract; view input MAY fail with `NotAView` |
| `contract{...}` invocation | Every operand represented by `AnyTensor` is an explicit `ReadableTensor` carrier | One `Tensor[...]` value | Logical reads followed by materialization |
| `einop{...}` invocation | Every operand represented by `AnyTensor` is an explicit `ReadableTensor` carrier | One or more `Tensor[...]` values | Logical reads followed by materialization |
| `get_at` | Readable source; `Take` coordinates additionally accept readable `i64` tensor carriers | `Tensor[...]` | Generalized gather materializes |
| `set_at` | Readable source and readable same-element values | `Tensor[...]` | Pure scatter materializes without mutating the source |
| `set_at!` | Concrete `&mut Tensor[T]` destination and readable same-element values | `Unit` | Writable destination; reference aliases observe commit, while values remain read-only snapshots |
| `map_axis` mapped callback parameters | Position-wise concrete `Tensor[T]` or `TensorView[T]`, preserved exactly | Stacked `Tensor[...]` | Observational map-and-stack; no carrier conversion |
| `grad` and `value_and_grad` differentiated parameter | Concrete `Tensor[D]` or `TensorView[D]`, preserved exactly in the returned callable | Gradient and accepted scalar primal output are owning `Tensor[...]` values | AD-lowerability is a graph property, not a carrier capability |

Rules:

1. `ReadableTensor` never implies affine view construction, materialization,
   mutability, or AD support.
2. `AffineTensor` is required in addition to `ReadableTensor` wherever success
   constructs a retained zero-copy view from an otherwise generic carrier.
3. A materializing operation MUST read `TensorView` operands directly and MUST
   NOT insert an intermediate `materialize()` call.
4. A mutable destination remains concrete `Tensor`; neither capability permits
   mutation through an abstract carrier.
5. `AnyTensor` is catalog notation for explicit readable-carrier parameters, as
   defined under Core Types. It is not a source union or erased runtime carrier.
6. `map_axis` and `grad` use their listed concrete-carrier mappings rather than
   an anonymous capability parameter because they produce callable values whose
   complete parameter types MUST remain source-visible.
7. v0.1 exposes no public custom-operation or custom-kernel callable. Such an
   extension therefore has no baseline carrier row.
8. The pure name-overlay APIs remain concrete owning-`Tensor` operations in
   v0.1. The catalog defines no implicit view-preserving overload for
   `TensorView`.
9. `grad` and `value_and_grad` accept only the three owning-`Tensor` primal
   result families listed in their section. A scalar `TensorView` primal result
   is not an additional accepted family.


### Names Overlay API

```eglur-catalog
fn Tensor[T].with_names(self: Tensor[T], map: NameOverlay) -> Res[Tensor[T], AxisError]
fn Tensor[T].rename(self: Tensor[T], map: Map[AxisName, AxisName]) -> Res[Tensor[T], AxisError]
```

Rules:

1. `with_names` interprets `map` as an atomic patch over the receiver's current `NameOverlay`; entries omitted from `map` MUST be preserved.
2. Existing entries at supplied indices MUST be replaced, and an empty map MUST be a no-op.
3. All patch keys MUST be bounds-checked before constructing the candidate overlay (`AXIS_OUT_OF_RANGE`).
4. Patch entries MUST be applied simultaneously, and only the final candidate overlay is checked for duplicate output names (`AXIS_DUPLICATE_NAME`).
5. A valid name swap MUST NOT fail because of a transient intermediate duplicate.
6. Deterministic validation order and duplicate selection follow `../axes.md`.
7. `with_names` cannot remove an overlay entry; v0.1 defines no direct name-removal API.
8. `rename` MUST fail on missing source names and duplicate output names (`AXIS_NAME_NOT_FOUND` / `AXIS_DUPLICATE_NAME`).
9. `with_names` SHOULD be metadata-only view update (no data buffer copy).
10. The `ReadableTensor` implementation for `TensorView` MUST resolve
    `axis_of()` against the retained snapshot's overlay without materializing the
    view.

Required in-place forms:

```eglur-catalog
fn Tensor[T].with_names!(self: &mut Self, map: NameOverlay) -> Res[Unit, AxisError]
fn Tensor[T].rename!(self: &mut Self, map: Map[AxisName, AxisName]) -> Res[Unit, AxisError]
```

Rules:

1. `with_names!` MUST use the same atomic patch and validation rules as `with_names`.
2. Any `Err(AxisError)` from `with_names!` or `rename!` MUST leave the receiver's overlay observationally unchanged.
3. Implementations MUST ensure unique header ownership before committing in-place metadata mutation.
4. Implementations MAY realize Rule 3 via copy-on-write header split or equivalent internal strategy.
5. In v0.1 baseline, `with_names!`/`rename!` MUST NOT fail solely due to internal shared-header representation produced by implementation optimizations.
6. `AXIS_ALIASED_HEADER` is reserved for non-baseline strict-alias extension profiles (see `../errors/runtime-codes.md`).


### Constructors

Pure constructors:

```eglur-catalog
fn zeros[T](dims: Nat...) -> Tensor[T]
fn ones[T](dims: Nat...) -> Tensor[T]
fn nan[f32](dims: Nat...) -> Tensor[f32]
```

Random constructors are bang:

```eglur-catalog
fn rand![f32](dims: Nat...) -> Tensor[f32]
```

The dedicated constructor dtype token `bool` selects source element type `Bool`; every numeric dtype token selects its same-spelled scalar source type.
The bracket-call surface on these constructors is the dedicated `DTypeBracketCallSuffix` from `../language/syntax.md`; it does not define general tensor or expression indexing.

Dedicated dtype-bracket resolution is closed:

1. The eligible canonical callable identities are the constructor declarations
   in this section. `zeros` and `ones` accept each supported `DType[T]` source
   type; `nan` and `rand!` accept only their catalog-declared `f32`
   specialization in v0.1.
2. Direct imports, import aliases, and public re-exports preserve canonical
   callable identity and therefore preserve bracket eligibility:

   ```eglur
   import std::tensor::ones as filled

   let x = filled[f32](2, 3)
   ```

3. A same-spelled local binding or module declaration shadows a lower-tier
   import and has its own identity. It does not inherit bracket eligibility.
4. The bracket argument is parsed as `TypeExpr`. The dedicated lowercase
   spelling `bool` denotes source type `Bool`; every other type name is resolved
   in the type namespace. The resulting type is normalized through transparent
   aliases before checking the selected constructor's accepted dtype family.
5. A normalized type outside that family, including `Tri`, an abstract numeric
   kind, or an unsupported user type, MUST fail with
   `CALL_ARG_TYPE_MISMATCH`.
6. An ordinary callee, including a shadowing declaration, MUST fail with
   `TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN` before ordinary call-argument binding.
7. Local runtime callable bindings do not acquire bracket eligibility merely by
   referring to an eligible constructor.

Dimension arguments are `Nat` values.
Integer literals in constructor argument position are contextually typed as `Nat`.
Signed runtime values MUST be converted with `Nat.from_i64` before they can be passed as dimensions.
Negative dynamic values therefore fail at that explicit conversion boundary with `DOMAIN_INVALID_VALUE`; tensor constructors MUST NOT define a separate negative-dimension runtime trap in v0.1.


### Pattern Literal Core and Dim Solver (`std::tensor::xform`)

All transform literals share a common pattern shell:

```text
<operands> -> <outputs>
```

Where:

1. `<operands>` is one or more axis lists separated by `,`.
2. `view`, `rearrange`, `inflate`, and `einop` accept one or more output axis
   lists separated by `,`.
3. `reduce` and `contract` accept exactly one output axis list.
4. Each axis list MAY be empty or contain one or more axis entries separated by whitespace.
5. An empty axis list denotes rank-0 (`shape=()`).

Catalog schema placeholders map to the source grammar in `../language/syntax.md` as follows:

1. `<operands>` is `TransformOperands` and `<operand>` is `TransformOperand`.
2. `<outputs>` is `TransformOutputs` and `<output>` is `TransformOutput`.
3. `<out_axes>` is `TransformOutput` in the `contract{...}` production.

Axis expression grammar for transform literals (`view`, `rearrange`, `inflate`, `reduce`, `einop`):

1. integer literals
2. dimension variables
3. `+`
4. `*`
5. parentheses

Axis-list entries for transform literals (`view`, `rearrange`, `inflate`, `reduce`, `einop`) are:

1. `AxisExpr` (single axis), or
2. axis-pack token:
   1. anonymous pack `...`,
   2. named pack `...name`.

Axis-pack rules:

1. Any pack matches zero or more concrete axes.
2. Anonymous pack `...` MUST appear at most once per transform literal.
3. Multiple variadic captures require named packs (`...p1`, `...p2`, ...).
4. Output-side named packs MUST be bound by at least one input-side named-pack occurrence; otherwise compilation MUST fail with `AXIS_PACK_UNBOUND`.
5. Repeated occurrences of one named pack MUST bind to identical axis sequences.

Operator precedence and associativity:

1. Parentheses bind first.
2. `*` binds tighter than `+`.
3. Equal-precedence operators associate left-to-right.

Operation schemas that support symbolic dimensions MUST expose:

```eglur-catalog
fn Schema.with_sizes(self: Self, sizes: Map[Str, Nat]) -> Self
```

`Schema` is the closed catalog family comprising `ViewSchema`, `RearrangeSchema`, `InflateSchema`, `ReduceSchema`, `ReduceBySchema`, and `EinopSchemaN`, the tensor schema values that admit symbolic dimension bindings.
`with_sizes` preserves the receiver's concrete schema family.

`with_sizes` binding-key rules:

1. Eligible keys are exactly the scalar dimension-variable identifiers that occur in any input or output `AxisExpr` of the receiver schema.
2. Variables that occur only on the output side are eligible keys.
3. Anonymous and named axis packs are not scalar dimension variables and are not eligible keys.
4. The map MAY bind any subset of eligible keys. Omitted variables remain solver inputs.
5. A supplied eligible binding is a constraint even when the same value could be inferred from operand shapes; a conflict fails with `INCONSISTENT_DIMS`.
6. Any ineligible key fails with `WITH_SIZES_UNKNOWN_KEY` before dimension solving begins.
7. A const-evaluable offending key is rejected at compile time. Otherwise, schema invocation returns `Err(...)` through the operation's existing error family with the same code.
8. If more than one offending key is present, the first key in `Map` insertion order is reported.

Dim solver rules:

1. All dimension variables in the pattern MUST resolve to `Nat` values before
   execution.
2. `.with_sizes(...)` bindings MUST be `Nat` values. Signed runtime values MUST
   be converted with `Nat.from_i64` before map construction; failed conversion
   uses `DOMAIN_INVALID_VALUE`.
3. For each candidate axis-pack partition, implementations MUST construct a
   finite set of equations `e = d`, where:
   1. `e` is an `AxisExpr` after explicit-binding substitution,
   2. `d` is a concrete `Nat` dimension from operand shape or an
      operation-specific shape/element-count constraint,
   3. output-only expressions contribute equations only where that operation's
      normative rules impose a concrete constraint.
4. Before solving, implementations MUST recursively constant-fold and apply
   the identities `x + 0 = x`, `0 + x = x`, `x * 1 = x`, `1 * x = x`, and
   `x * 0 = 0 * x = 0`. Arithmetic used only for comparison MAY saturate at
   `d + 1`; it MUST NOT overflow or wrap. Identical normalized equations MUST
   be deduplicated before connected-component classification, preserving the
   first equation in operand and source order.
5. Implementations MUST then run this fixed-point procedure:
   1. reject a false ground equation with `INCONSISTENT_DIMS`,
   2. select the first remaining equation in the Rule 4 order that contains
      exactly one distinct unresolved variable `x` and solve `e(x) = d` over
      `Nat`,
   3. substitute that solved value through every equation before selecting
      another equation,
   4. repeat until no variable is solved; the procedure assigns at most the
      finite number of distinct variables.
6. After Rule 4 simplification, a non-constant `e(x)` made only from `Nat`
   literals, `x`, `+`, and `*` is strictly increasing over `Nat`. Its unique
   candidate MUST be found by overflow-safe bracketing and binary search, with
   intermediate comparisons saturated at `d + 1`:
   1. exact equality fixes `x`,
   2. no exact `Nat` root fails with `INCONSISTENT_DIMS`,
   3. repeated occurrences such as `x * x` remain one-variable equations and
      are included in this rule.
7. A variable removed from every equation by Rule 4 and not fixed by
   `.with_sizes(...)` is free. If all other equation components have at least
   one solution, the resulting multiple-solution family MUST fail with
   `AMBIGUOUS_DIMS`.
8. The baseline procedure has one multivariate arithmetic certificate. An
   otherwise isolated unresolved component exactly of the form
   `c * x_1 * ... * x_k = d`, where `c > 0`, `k >= 2`, each `x_i` is distinct
   and occurs exactly once, and the variables occur in no other equation, MUST
   be classified without factor search:
   1. if `c` does not divide `d`, fail with `INCONSISTENT_DIMS`,
   2. if `d / c == 1`, the unique solution fixes every `x_i` to `1`,
   3. if `d / c == 0` or `d / c > 1`, fail with `AMBIGUOUS_DIMS`.
9. Any unresolved connected multivariate equation component not classified by
   Rules 7-8 MUST produce `DIM_BINDING_REQUIRED`. Its `variables` data field
   lists that component's unresolved variables in first source-occurrence
   order. Binding enough of those variables to leave only ground or univariate
   components MUST permit the procedure to continue.
10. Independent equation components combine as follows:
    1. a proven inconsistent component makes the candidate inconsistent,
    2. otherwise, any `DIM_BINDING_REQUIRED` component makes the candidate
       require bindings,
    3. otherwise, any component with multiple solutions makes the candidate
       ambiguous,
    4. otherwise, the candidate has one complete solution.
11. A successful solve MUST be unique and deterministic. Non-unique solves MUST
    be rejected even when reduced or eliminated outputs would still be unique.
12. More than one anonymous pack occurrence MUST fail with
    `AXIS_PACK_ANON_DUPLICATE` before candidate enumeration.
13. For one concrete operand axis list of rank `R`, containing `F` fixed
    `AxisExpr` entries and `P > 0` pack occurrences, the parser-independent
    partition candidate count is bounded by the weak-composition count
    `choose(R - F + P - 1, P - 1)` when `R >= F`; it is zero when `R < F`.
    With no pack occurrence, exactly one candidate exists when `R == F`.
14. Implementations MUST enumerate or memoize only those finite rank-partition
    candidates. Repeated named-pack occurrences reject candidates whose bound
    concrete axis sequences differ. Output named packs reuse their input
    bindings and do not add partition choices.
15. Candidate outcomes combine as follows:
    1. two distinct complete solutions, or one candidate already proven to have
       multiple solutions, produce `AMBIGUOUS_DIMS`,
    2. one complete solution plus any binding-required candidate, or no complete
       solution plus at least one binding-required candidate, produces
       `DIM_BINDING_REQUIRED`,
    3. one complete solution with every other candidate inconsistent succeeds,
    4. all candidates inconsistent produce `INCONSISTENT_DIMS`.
16. Implementations MAY stop candidate enumeration once two distinct complete
    solutions or another sound multiple-solution certificate has been found.
    They MUST NOT materialize every solution merely to report ambiguity.
17. The solver MUST NOT enumerate the `Nat` domain, enumerate divisors, inspect
    tensor elements, execute a backend, or invoke a general multivariate
    integer/Diophantine/SMT solver. The finite `Nat` representation bound does
    not permit exhaustive value search.

Every schema in this section MUST use the recursive `AxisExpr` coordinate
mapping in `../language/tensor-model.md`. That mapping exclusively defines
sum-segment order, rightmost-factor-fastest product order, zero-product
behavior, repeated-occurrence handling, written-tree-directed distributive and
factoring transport, and overflow-safe map construction. It also defines the
closed structural mapping relation and its compile-time
`AXIS_MAPPING_UNSUPPORTED` failure. The same section defines the multi-input
boundary at which `rearrange` fails with `SEGMENT_STRADDLE`; operation-specific
rules below MUST NOT reinterpret those logical coordinates from physical
strides or perform broader algebraic inference.


### `view{...}` (`std::tensor::xform`)

`view` is a zero-copy, affine-only transform.

```eglur-catalog
schema view{ <operands> -> <outputs> } -> ViewSchema
callable ViewSchema[C: ReadableTensor + AffineTensor](
  input: C
) -> Res[ViewResult[C::Elem], TransformError]
```

Rules:

1. v0.1 `view` MUST be unary.
2. Multi-input `view` MUST fail with `MULTI_INPUT_NOT_ALLOWED`.
3. On success, outputs MUST be `TensorView` values over the same retained base-buffer
   snapshot.
4. Data copy/memcpy during `view` construction is forbidden.
5. If affine view mapping cannot represent the requested transform, fail with `NOT_A_VIEW`.
6. Unary multi-output `view` is allowed only when outputs form a non-overlapping partition of the consumed input region.
7. Retaining that snapshot MUST NOT create observable write-through aliasing; source
   mutation and view copying follow `../language/tensor-model.md`.
8. `ViewResult[T]` is normalized before source type checking:
   1. a one-output schema returns `TensorView[T]`,
   2. a schema with `N >= 2` outputs returns an `N`-tuple of
      `TensorView[T]` values in right-hand-side order.
9. `ViewResult` is catalog-only and MUST NOT appear in the resulting source
   signature.
10. A schema outside the closed structural mapping relation MUST fail at
    compile time with `AXIS_MAPPING_UNSUPPORTED`.
11. Statically decidable schema, dimension, and affine-mapping failures use
    the compile channel with their specific cause code. A failure that depends
    on runtime shape or size data returns `Err(TransformError)` with that same
    diagnostic cause.


### `rearrange{...}` (`std::tensor::xform`)

`rearrange` is total structural transform with materialization.

```eglur-catalog
schema rearrange{ <operands> -> <outputs> } -> RearrangeSchema
callable RearrangeSchema(inputs: AnyTensor...) -> Res[RearrangeResult, TransformError]
```

Rules:

1. `rearrange` MAY be multi-input and multi-output.
2. Schema operand count fixes invocation arity. `inputs: AnyTensor...` expands
   positionally to fresh carriers `C0, ..., Cn`, each bounded by
   `ReadableTensor`, as defined under Core Types.
3. For each right-hand-side output `oi`, the rearrange contributor-provenance
   rules in `../language/tensor-model.md` define a non-empty ordered input-position
   list `Pi`. Provenance is determined from the schema and does not depend on
   runtime axis lengths or values.
4. Ordinary argument binding and carrier inference first fix each `Ci`.
   Call-site substitution and associated-projection normalization then apply to
   every `Ci::Elem`.
5. Each `oi` has element dtype `Ui` only when every normalized `Cj::Elem` for
   `j` in `Pi` is the same canonical type. Repeated occurrences of one canonical
   projection are equal; distinct unresolved canonical projections are not. If
   no such `Ui` exists, compilation MUST fail with
   `REARRANGE_OUTPUT_DTYPE_MISMATCH`.
6. After Rules 4-5, `RearrangeResult` is normalized before checking result
   assignability:
   1. a one-output schema returns `Tensor[U0]`,
   2. a schema with `N >= 2` outputs returns an `N`-tuple of `Tensor[Ui]`
      values in right-hand-side order.
7. `AnyTensor` and `RearrangeResult` are catalog-only and MUST NOT appear in the
   resulting source signature. `rearrange` performs no implicit dtype promotion
   or cast.
8. Every output MUST own a fresh materialized buffer.
9. Lowering MUST include an explicit realization/materialization barrier (for
   example `REALIZE` plus a copy plan) and MUST NOT be optimized into view-only
   semantics. A non-affine distributive/factoring map MUST encode the shared
   coordinate relation explicitly, for example with `IOTA`/index arithmetic and
   `GATHER`; backend shape inference MUST NOT choose its branch order.
10. `rearrange` MUST preserve global element count:
   1. If output numel is greater than input numel, fail with `NUMEL_MISMATCH_GROW`.
   2. If output numel is smaller than input numel, fail with `NUMEL_MISMATCH_SHRINK`.
11. `rearrange` MUST NOT perform implicit duplication or reduction.
12. A schema outside the closed structural mapping relation MUST fail at
    compile time with `AXIS_MAPPING_UNSUPPORTED`.
13. Statically decidable schema, axis, dimension, segment, and numel failures
    precede contributor dtype equality. Dtype validation visits outputs in
    right-hand-side order. Within one output it compares contributors in
    input-operand order against the first contributor and reports the first
    unequal pair.
14. Statically decidable schema, dimension, segment, and numel failures use the
    compile channel with their specific cause code. A failure that depends on
    runtime shape or size data is checked only during execution and returns
    `Err(TransformError)` with that same diagnostic cause; such a potential
    runtime failure does not suppress a compile-time dtype failure. Contributor
    dtype equality is always statically decided.


### `inflate{...}` (`std::tensor::xform`)

`inflate` is explicit duplication transform (einops-style repeat class).

```eglur-catalog
schema inflate{ <operand> -> <output> } -> InflateSchema
callable InflateSchema[C: ReadableTensor](
  input: C
) -> Res[Tensor[C::Elem], TransformError]
```

Rules:

1. v0.1 `inflate` MUST be unary and single-output.
2. `inflate` MUST materialize a fresh owning tensor.
3. Output element count MUST equal input count multiplied by repeat factors (including zero-valued factors).
4. Zero repeat factors are valid and MUST produce empty output axes.
5. Literal output factors and symbolic factors fixed by `.with_sizes(...)` MUST be equivalent (for example `inflate{ -> 3 }` and `inflate{ -> k }.with_sizes(["k": 3])`).
6. An output-only factor coordinate selects a duplicate of the matching input
   coordinate. Product factors follow the shared rightmost-fastest order; for
   example, `inflate{ i -> (i * r) }` emits all `r` copies of one `i`
   coordinate before advancing `i`.
7. Lowering MUST include explicit realization/materialization and MUST NOT be treated as view-only transform.
8. The callable preserves the input element dtype and always returns an owning
   `Tensor[C::Elem]`; it reads `TensorView` inputs directly without an
   intermediate materialization.
9. A schema outside the closed carry-and-repeat mapping relation MUST fail at
   compile time with `AXIS_MAPPING_UNSUPPORTED`.
10. Statically decidable schema or dimension failures use the compile channel
   with their specific cause code. A failure that depends on runtime shape or
   size data returns `Err(TransformError)` with that same diagnostic cause.


### `reduce{...}` (`std::tensor::xform`)

`reduce` performs axis elimination with default reducer `sum`.

```eglur-catalog
schema reduce{ <operand> -> <output> } -> ReduceSchema
fn ReduceSchema.reduce_by(self: Self, args: ReduceByArg...) -> ReduceBySchema
callable ReduceSchema[C: ReadableTensor](t: C) -> Res[Tensor[C::Elem], Union[AxisError, ShapeError, DTypeError, NumericError]]
callable ReduceBySchema[C: ReadableTensor + AffineTensor](
  t: C
) -> Res[ReduceByResult, Union[ReduceByErrors, TransformError]]
```

The `args: ReduceByArg...` pack is constrained by the exact source
`ReduceByArgs` production: it is either one bare reducer expression or a
non-empty sequence of `ReduceByPhase` entries. Mixed bare-reducer/phase packs
and empty packs are invalid for the canonical `ReduceSchema.reduce_by` form
(`REDUCE_BY_ARGUMENTS_INVALID`). The parser retains an ordinary member-call
candidate until receiver identity resolution; a non-`ReduceSchema` receiver
continues to use ordinary call grammar and fails with the same diagnostic if a
retained phase-only candidate has no ordinary `ArgList` interpretation.
`ReduceByArg` is a catalog-only argument family; it does not introduce a
source-visible type or runtime erased argument representation.

`ReduceAxisGroup` is the catalog-only argument family represented exactly by
the source `ReduceByPhaseAxes` production. It resolves to one or more axis
identifiers bound by the receiver schema and does not construct a runtime
`Vec[AxisSel]`.

Rules:

1. v0.1 `reduce` MUST be unary and single-output.
2. Axes present only on the left side are reduced axes.
3. Reduced axis iteration order MUST follow left-side axis order. A non-empty
   fixed-width integer default reduction MUST additionally use the checked
   result-dtype left fold in `conventions.md`; this does not change the subtensor
   view contract for `reduce_by`.
4. `reduce_by` has two v0.1 surface forms:
   1. single reducer form: `.reduce_by(f)`,
   2. ordered phase form with `N >= 1`:
      `.reduce_by((axes_1, f_1), ..., (axes_N, f_N))`.
5. The single reducer form MUST normalize to one phase over every reduced axis
   in left-side order.
6. An ordered phase plan MUST pass compile-time axis-partition validation before
   callback-chain typing:
   1. each `axes_i` MUST refer only to reduced axes from the schema
      (`REDUCE_BY_PHASE_NON_REDUCED_AXIS` on violation),
   2. phase axis sets MUST be pairwise disjoint
      (`REDUCE_BY_PHASE_OVERLAP` on violation),
   3. the union of all phase axis sets MUST equal the full reduced-axis set
      (`REDUCE_BY_PHASE_INCOMPLETE` on partial coverage).
7. After the invocation input fixes concrete carrier `C`, the normalized
   `N`-phase plan MUST be typed in written left-to-right order:
   1. `T_0` is `C::Elem`,
   2. phase `i` is specialized under expected input `TensorView[T_i]`; its
      resulting callable type MUST be
      `Fn(TensorView[T_i]) -> Res[U_i, F_i]`,
   3. `U_i` MUST inhabit the implementation's documented `DType[U_i]` family,
      and `T_(i+1)` is exactly `U_i`,
   4. `E_i` is exactly the normalized error type argument `F_i` from that
      specialized callback result; contextual assignment to a wider error union
      MUST NOT widen `E_i`,
   5. `T_(i+1)` and `E_i` become the expected input element type and error
      member for the next phase,
   6. an incompatible callable parameter, non-`Res` result, or successful
      payload without a `DType` witness MUST fail with
      `CALL_ARG_TYPE_MISMATCH`; an effectful phase callable MUST fail with
      `PURE_COMBINATOR_EFFECTFUL_CALLBACK`,
   7. type checking stops at the first invalid phase in source order.
8. `ReduceSchema.reduce_by` is the only expression-local schema specialization
   in v0.1:
   1. it applies only when `reduce_by` construction and invocation form one
      complete expression and an inline closure-literal phase callback has an
      unannotated parameter whose expected `TensorView[T_i]` type depends,
      directly or through earlier phase results, on invocation root `T_0`,
   2. the invocation carrier is checked first and supplies `C::Elem` as `T_0`;
      after substituting `T_0`, callbacks are checked left-to-right by Rule 7,
   3. canonical `input.then(schema)` and `(input,).apply(schema)` forms are
      desugared before this classification and therefore count when `schema` is
      the complete `reduce{...}.reduce_by(...)` expression,
   4. result and error types are inferred only by ordinary forward callback-body
      typing; invocation result context MUST NOT reverse-constrain an
      independent callback result or error variable,
   5. without a same-expression invocation, construction-site callback types
      MAY fix `T_0` only from the first normalized phase's exact
      `TensorView[T_0]` input type, supplied by an explicit inline-closure
      parameter annotation or a fully specialized concrete callable. Once
      fixed, Rule 7 checks the remaining chain forward; a later phase, callback
      result context, or later schema invocation MUST NOT infer `T_0` backward,
   6. `let op = reduce{...}.reduce_by(...)` MUST be fully determined while
      checking that initializer. A later `op(input)` call cannot add constraints;
      an unresolved input-dependent callback parameter or other callback
      variable MUST fail with `BINDING_TYPE_UNDETERMINED` at the binding,
   7. before `BINDING_TYPE_UNDETERMINED` is emitted, axis-partition diagnostics
      from Rule 6 and callback-family, arity, result-shape, or effect violations
      from Rule 7 that are decidable without the missing `T_0` MUST be reported
      at their ordinary source sites,
   8. a stored schema whose callback chain is fully determined at construction
      is an ordinary monomorphic `ReduceBySchema` value and MAY be invoked
      repeatedly with carriers compatible with its fixed `T_0`,
   9. named or generic functions, overload sets, placeholder closures, hole-based
      partial applications, and independently unresolved callback variables
      remain subject to ordinary construction-site inference,
   10. these rules do not propagate constraints into `map_axis` or any other
      callable-returning API.
9. Before source result checking, the catalog-only result families MUST
   normalize as follows:
   1. `ReduceByResult` becomes the owning result type `Tensor[T_N]`,
   2. `ReduceByErrors` becomes the normalized error union
      `E_0 | ... | E_(N-1)`,
   3. ordinary error-union flattening and duplicate elimination apply across
      the phase errors and `TransformError`,
   4. both result families MUST be eliminated from source-visible signatures.
10. Phase `i` removes exactly its axis set from the current logical tensor. For
   each coordinate of the remaining axes, it invokes `f_i` exactly once with a
   `TensorView[T_i]` over that coordinate's phase subtensor. The view's axis order
   follows current left-side order, not the spelling order of the axis set.
11. The `Ok` values from phase `i` define the logical intermediate owning
    `Tensor[T_(i+1)]` consumed by phase `i+1`. An implementation MAY elide
    physical intermediate storage only when every later callback observes the
    same retained values, shape, axis order, and error behavior.
12. A phase returning `Err(e)` MUST abort the complete `reduce_by` invocation,
    propagate `e` without wrapping, invoke no later phase, and expose no partial
    result.
13. Empty phase subtensors MUST still be passed to the phase callback.
14. Empty-slice behavior is defined by the callback result:
   1. if the callback returns `Ok(...)`, that value is used for the output coordinate,
   2. if the callback returns `Err(...)`, reduction fails through ordinary `Res` propagation.
15. Implementations MAY parallelize callback invocations across output
    coordinates within one phase. A callback in phase `i+1` MUST NOT run until
    phase `i` has completed successfully.
16. v0.1 does not assume algebraic reducer laws (for example
    identity/associativity/commutativity) for user-supplied callbacks.
    Implementations MUST NOT split and reassociate one phase subtensor unless a
    future algebraic reducer contract is standardized.
17. Ordered phase form is the canonical v0.1 customization surface for multi-stage reduction planning.
18. v0.1 deliberately uses ordered phase tuples rather than named/unordered reducer maps:
   1. phase order is observable in lowering/planning and therefore part of semantics,
   2. unordered reducer maps/dictionaries MUST NOT be exposed because phase order would be ambiguous for non-commuting reducers,
   3. any extension that adds named ergonomic sugar MUST preserve this ordered phase sequence as the canonical internal model.
19. `reduce{...}.by(...)`, `.contract_by(...)`, and other `.by` member forms are undefined in v0.1 and MUST be rejected with `METHOD_NOT_FOUND`.
20. Statically decidable schema and dimension failures use the compile channel
    with their specific cause code. A failure that depends on runtime shape or
    size data returns `Err(TransformError)` with that same diagnostic cause.
21. The default reducer's empty-domain behavior is the additive-identity rule in
    `conventions.md`. Rules 13-14 remain the complete empty-slice contract for
    user-supplied `reduce_by` callbacks.


### Primitive Operations (`std::tensor::prim`)

Catalog blocks in this section declare callables in `std::tensor::prim`.
Binary elementwise `max(x, y)` and reduction `max(x, axis, keepdims)` form the explicit `max` overload set.

`NumericError` in this file's generic signatures is a dtype-conditional error member:

1. it is retained in the instantiated source-visible error union when the operation executes fixed-width integer arithmetic that can fault, including arithmetic reached through `bool` promotion to `u8`;
2. it is omitted when statically known operand and promoted execution dtypes use only floating or complex IEEE-754 value semantics;
3. an operand dtype unsupported by the selected operation fails through `DTypeError`, not `NumericError`;
4. this specialization applies to elementwise primitives, unary `neg`/`abs`, `sum` and default `reduce`, `contract`, and `einop`.

Elementwise (minimum):

```eglur-catalog
fn neg[C: ReadableTensor](x: C) -> Res[Tensor[C::Elem], Union[DTypeError, NumericError]]
fn abs[C: ReadableTensor](x: C) -> Res[Tensor[C::Elem], Union[DTypeError, NumericError]]
fn exp[C: ReadableTensor](x: C) -> Res[Tensor[C::Elem], DTypeError]
fn log[C: ReadableTensor](x: C) -> Res[Tensor[C::Elem], DTypeError]
fn sqrt[C: ReadableTensor](x: C) -> Res[Tensor[C::Elem], DTypeError]
fn tanh[C: ReadableTensor](x: C) -> Res[Tensor[C::Elem], DTypeError]

fn add[X: ReadableTensor, Y: ReadableTensor](x: X, y: Y) -> Res[Tensor[Promote[X::Elem, Y::Elem]], Union[ShapeError, DTypeError, NumericError]]
fn sub[X: ReadableTensor, Y: ReadableTensor](x: X, y: Y) -> Res[Tensor[Promote[X::Elem, Y::Elem]], Union[ShapeError, DTypeError, NumericError]]
fn mul[X: ReadableTensor, Y: ReadableTensor](x: X, y: Y) -> Res[Tensor[Promote[X::Elem, Y::Elem]], Union[ShapeError, DTypeError, NumericError]]
fn div[X: ReadableTensor, Y: ReadableTensor](x: X, y: Y) -> Res[Tensor[Promote[X::Elem, Y::Elem]], Union[ShapeError, DTypeError, NumericError]]
fn rem[X: ReadableTensor, Y: ReadableTensor](x: X, y: Y) -> Res[Tensor[Promote[X::Elem, Y::Elem]], Union[ShapeError, DTypeError, NumericError]]
overload fn max[X: ReadableTensor, Y: ReadableTensor](x: X, y: Y) -> Res[Tensor[Promote[X::Elem, Y::Elem]], Union[ShapeError, DTypeError]]
fn min[X: ReadableTensor, Y: ReadableTensor](x: X, y: Y) -> Res[Tensor[Promote[X::Elem, Y::Elem]], Union[ShapeError, DTypeError]]
overload fn where[X: ReadableTensor, Y: ReadableTensor](cond: Tensor[Bool], x: X, y: Y) -> Res[Tensor[Promote[X::Elem, Y::Elem]], Union[ShapeError, DTypeError]]
overload fn where[X: ReadableTensor, Y: ReadableTensor](cond: TensorView[Bool], x: X, y: Y) -> Res[Tensor[Promote[X::Elem, Y::Elem]], Union[ShapeError, DTypeError]]

overload fn wrapping_add[C: ReadableTensor](x: C, y: Tensor[C::Elem]) -> Res[Tensor[C::Elem], Union[ShapeError, DTypeError]]
overload fn wrapping_add[C: ReadableTensor](x: C, y: TensorView[C::Elem]) -> Res[Tensor[C::Elem], Union[ShapeError, DTypeError]]
overload fn wrapping_sub[C: ReadableTensor](x: C, y: Tensor[C::Elem]) -> Res[Tensor[C::Elem], Union[ShapeError, DTypeError]]
overload fn wrapping_sub[C: ReadableTensor](x: C, y: TensorView[C::Elem]) -> Res[Tensor[C::Elem], Union[ShapeError, DTypeError]]
overload fn wrapping_mul[C: ReadableTensor](x: C, y: Tensor[C::Elem]) -> Res[Tensor[C::Elem], Union[ShapeError, DTypeError]]
overload fn wrapping_mul[C: ReadableTensor](x: C, y: TensorView[C::Elem]) -> Res[Tensor[C::Elem], Union[ShapeError, DTypeError]]
fn wrapping_cast[C: ReadableTensor, U](x: C, dtype: DType[U]) -> Res[Tensor[U], CastError]

fn astype[C: ReadableTensor, U](x: C, dtype: DType[U]) -> Res[Tensor[U], CastError]
fn bitcast[C: ReadableTensor, U](x: C, dtype: DType[U]) -> Res[Tensor[U], CastError]
```

The unary transcendental family `exp`, `log`, `sqrt`, and `tanh` has one closed
v0.1 dtype contract:

1. Accepted element dtypes are exactly `f16`, `bf16`, `f32`, `f64`,
   `complex64`, and `complex128`.
2. A successful call preserves the input element dtype and returns an owning
   `Tensor[C::Elem]`. An implementation MAY use a wider internal evaluation
   dtype, but it MUST round each result back to `C::Elem`.
3. `Bool` and fixed-width integer inputs MUST return `Err(DTypeError)` through
   the declared value channel, including when the rejected dtype is statically
   known. These calls MUST NOT implicitly use `FloatLift`, choose a default
   floating dtype, or fail with `NumericError`.
4. Real inputs remain real. A real domain-invalid input produces NaN in the
   same dtype and MUST NOT implicitly produce a complex result.
5. Complex inputs use the standard analytic `exp`, `log`, `sqrt`, and `tanh`
   functions. `log` and `sqrt` use their principal values. `log` uses a
   principal argument in `[-pi, pi]`; on the negative-real branch cut, the
   argument sign follows the input imaginary component's signed zero. `sqrt`
   chooses the root with nonnegative real part and, when that part is zero, an
   imaginary part with the input imaginary component's sign.

For each supported real floating dtype, the following special-value table is
normative. Every operation propagates NaN.

| Operation | Required real special-value behavior |
| --- | --- |
| `exp` | `exp(+Inf) = +Inf`; `exp(-Inf) = +0`; finite overflow produces `+Inf` |
| `log` | `log(+Inf) = +Inf`; `log(+0) = log(-0) = -Inf`; every negative nonzero real, including `-Inf`, produces NaN |
| `sqrt` | `sqrt(+Inf) = +Inf`; `sqrt(+0) = +0`; `sqrt(-0) = -0`; every negative nonzero real, including `-Inf`, produces NaN |
| `tanh` | `tanh(+Inf) = +1`; `tanh(-Inf) = -1`; `tanh(+0) = +0`; `tanh(-0) = -0` |

For integer dtypes, `div` produces the quotient truncated toward zero and
`rem` produces its dividend-signed remainder under the scalar-shared law in
`conventions.md`. The remainder primitive is named `rem`; `mod` is not a v0.1
alias or callable.

Every `ReadableTensor` parameter above preserves its concrete source carrier for
argument checking and reads its logical values directly. Every successful
result is an owning `Tensor`; these operations MUST NOT materialize a view as a
separate intermediate step. This carrier rule does not otherwise constrain
legal storage reuse under ordinary tensor value semantics.

The two `where` overloads close the condition dtype to `Bool` for both public
carriers without introducing associated-type equality constraints. The paired
wrapping overloads similarly require the second operand element type to be the
first carrier's `C::Elem` while accepting either public readable carrier.

`wrapping_add`, `wrapping_sub`, `wrapping_mul`, and `wrapping_cast` accept integer dtypes only (`i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`).
The destination witness `DType[U]` participates in ordinary type-argument inference and fixes the successful cast result type to `Tensor[U]`. A cast does not erase the destination dtype, and v0.1 has no runtime-selected erased-dtype cast overload.

Elementwise binary ops MUST compute their common result shape with the trailing-axis
baseline broadcasting algebra in `../language/tensor-model.md`. `where` MUST apply
the same N-ary algebra directly to `cond`, `x`, and `y`.

Once operand axis metadata is valid, an incompatible broadcast MUST fail through
`ShapeError` before dtype resolution or scalar numeric execution. Axis names do not
participate in broadcast compatibility. Shape alignment MUST NOT by itself perform
an implicit dtype conversion or change an operand's logical values.

Elementwise binary numeric dtype resolution MUST follow `Promote(A,B)` from `conventions.md`.

Expression-level `/.` MUST use the fractional-division carrier rule `FDivCarrier(A,B)` from `conventions.md` (not `Promote(A,B)`) and MUST follow floating/complex IEEE value semantics for divide-by-zero.

Default name propagation for elementwise ops MUST be left-biased and shape-aligned.

Numeric runtime edge behavior for primitive arithmetic and casting MUST follow `conventions.md` exactly:

1. fixed-width integer overflow is checked (`INT_OVERFLOW`);
2. integer `div`/`rem` by zero are runtime errors (`INT_DIV_BY_ZERO`, `INT_REM_BY_ZERO`);
3. floating arithmetic is IEEE-754 value semantics (Inf/NaN propagation, signed-zero preservation, no integer-style overflow trap);
4. `wrapping_add`/`wrapping_sub`/`wrapping_mul` are the explicit modular-arithmetic escape hatch and MUST use two's-complement modulo semantics;
5. `wrapping_cast` MUST follow integer-only modulo cast rules in `conventions.md` and fail with `CAST_INVALID_DTYPE_PAIR` for illegal dtype pairs;
6. `bitcast` MUST follow equal-bit-width reinterpret rules in `conventions.md` and fail with `CAST_INVALID_DTYPE_PAIR` for illegal dtype pairs;
7. `astype` MUST follow checked value-cast rules in `conventions.md` and fail with `CAST_INVALID_VALUE` on invalid numeric cast input;
8. unary transcendental execution MUST follow the closed dtype and
   special-value contract above;
9. unary real-domain primitive execution MUST NOT implicitly promote to complex dtype;
10. binary mixed-domain primitive execution (including real+complex) MUST follow `Promote(A,B)` from `conventions.md`.

Reduce:

```eglur-catalog
fn sum[C: ReadableTensor](x: C, axis: AxisSel | Vec[AxisSel], keepdims: Bool = false)
  -> Res[Tensor[C::Elem], Union[AxisError, ShapeError, DTypeError, NumericError]]
overload fn max[C: ReadableTensor](x: C, axis: AxisSel | Vec[AxisSel], keepdims: Bool = false)
  -> Res[Tensor[C::Elem], Union[AxisError, ShapeError, DTypeError]]
fn mean[C: ReadableTensor](x: C, axis: AxisSel | Vec[AxisSel], keepdims: Bool = false)
  -> Res[Tensor[C::Elem], Union[AxisError, ShapeError, DTypeError]]
```

Reduction dtype rules:

1. `sum` and reduction-form `max` return the same element dtype as the input tensor when the dtype is supported by the operation:
   1. `sum` on `bool` is unsupported in v0.1 and MUST return `Err(DTypeError)`.
2. `mean` is defined only for floating and complex tensor dtypes (`f16`, `bf16`, `f32`, `f64`, `complex64`, `complex128`).
3. `mean` on `bool` or integer tensor dtypes MUST return `Err(DTypeError)`.
4. Successful `mean` returns the same element dtype as the input tensor.
5. `mean` MUST NOT implicitly apply `FloatLift`, preserve integer dtype with rounding/truncation, or otherwise hide a dtype conversion.
6. Integer `sum`, including the default reducer used by `reduce{...}`, MUST use
   the canonical reduced-axis traversal and checked left fold in
   `conventions.md`. Every accumulator step executes in `T`; the first
   non-representable step MUST return `Err(NumericError)` with cause
   `INT_OVERFLOW`.
7. Integer `neg` and `abs` MUST return `Err(NumericError)` with cause `INT_OVERFLOW` for the minimum signed value of `T`.
8. After selector resolution, the selected-axis reduction volume and all empty
   results MUST follow `conventions.md`:
   1. empty `sum` uses the additive identity in `C::Elem`,
   2. empty `mean` preserves `C::Elem` and produces NaN at every existing output
      coordinate,
   3. empty reduction-form `max` fails with `REDUCTION_EMPTY_DOMAIN`,
   4. an empty `Vec[AxisSel]` is an owning no-op after ordinary dtype validation.

Movement:

```eglur-catalog
fn permute[C: ReadableTensor + AffineTensor](x: C, order: Vec[AxisIndex]) -> Res[TensorView[C::Elem], Union[AxisError, ShapeError]]
overload fn reshape[T](x: Tensor[T], new_shape: Vec[Nat]) -> Res[TensorView[T], ShapeError]
overload fn reshape[T](x: TensorView[T], new_shape: Vec[Nat]) -> Res[TensorView[T], Union[ShapeError, NotAView]]
fn unsqueeze[C: ReadableTensor + AffineTensor](x: C, axis: AxisIndex) -> Res[TensorView[C::Elem], AxisError]
fn squeeze[C: ReadableTensor + AffineTensor](x: C, axis: AxisIndex) -> Res[TensorView[C::Elem], Union[AxisError, ShapeError]]
fn expand[C: ReadableTensor + AffineTensor](x: C, new_shape: Vec[Nat]) -> Res[TensorView[C::Elem], ShapeError]
fn slice[C: ReadableTensor + AffineTensor](x: C, ranges: Vec[RangeSpec]) -> Res[TensorView[C::Elem], IndexError]
```

Every successful movement call above MUST:

1. return a read-only `TensorView[C::Elem]` (or `TensorView[T]` for a concrete
   `reshape` overload) over the input carrier's retained base-buffer snapshot;
2. construct only affine logical metadata and stride mappings;
3. perform no element-buffer allocation, copy, or memcpy;
4. preserve the ordinary `TensorView` retention and mutation-isolation semantics;
5. require an explicit `TensorView.materialize()` call only before the result is
   passed to an API whose declared boundary specifically requires owning
   `Tensor[T]`.

These guarantees apply uniformly to permutation, valid reshape, singleton-axis
insertion or removal, directed expansion, and range-only slicing. Implementations
MUST choose internal owning-tensor representations that can satisfy every valid
primitive movement call on `Tensor[T]` without copying. A valid
`reshape(Tensor[T], ...)` MUST NOT fail only because an implementation would
otherwise need to copy. `reshape(TensorView[T], ...)` MUST return
`Err(NotAView)` when the requested logical order cannot be represented as an
affine view over the retained snapshot; it MUST NOT copy or materialize instead.
Other movement calls compose over either public affine carrier and MUST NOT
reject `TensorView` solely because it is not owning.

Both `reshape` overloads MUST validate `new_shape` and total element count
before checking affine representability. An invalid target shape returns
`Err(ShapeError)` for either carrier. `Err(NotAView)` is considered only for a
shape-valid `TensorView` request whose logical ordering cannot be represented
by one affine mapping.

`reshape(x, new_shape)` MUST preserve the complete axis-name overlay only when
`new_shape` is exactly equal to `shape(x)`. Every other successful reshape MUST
return an empty overlay, even when some dimensions appear unchanged or the
reshape only inserts or removes singleton axes. Primitive reshape MUST NOT infer
partial axis provenance; mapping-explicit alternatives are listed in
`../axes.md`.

`expand(x, new_shape)` MUST apply the baseline directed-expansion rule from
`../language/tensor-model.md` with `shape(x)` as the source and `new_shape` as the
target. Its successful result shape is exactly `new_shape`. Target rank smaller
than source rank, or any source dimension that is neither equal to the aligned
target dimension nor `1`, MUST return `Err(ShapeError)`. A size-1 source axis MAY
expand to target size `0`; a size-0 source axis MUST NOT expand to a nonzero size.
After successful expansion, every source axis name MUST follow its right-aligned
source axis to the corresponding output axis; logically prepended leading axes
MUST be unnamed. Size expansion does not drop a source axis name. The exact
overlay mapping is defined in `../axes.md`; the movement rules above define its
carrier and materialization guarantees separately from shape compatibility.

`RangeSpec` is the range-only indexing subset used by `slice`: each item MUST
be `IndexItem::All`, `IndexItem::Slice(...)`, or `IndexItem::Rest`.
`IndexItem` is exported by `std::tensor::index`; its variants are valid wherever
`RangeSpec` is required but are not standalone import symbols.
`slice` MUST apply the `IndexSpec` arity and `Rest` canonicalization defined
below before range normalization. It MUST preserve rank, axis order, and
axis-name overlays for every canonical `IndexItem::All` or
`IndexItem::Slice` axis. `slice` MUST reject `IndexItem::At`,
`IndexItem::Take`, and `IndexItem::NewAxis`; generalized gather/scatter remains
the responsibility of `std::tensor::index`.


### Contract API (`std::tensor::contract`)

```eglur-catalog
schema contract{ <operands> -> <out_axes> } -> OpSchemaN
callable OpSchemaN(inputs: AnyTensor...) -> Res[ContractResult, Union[ContractError, NumericError]]
```

For each call, `inputs: AnyTensor...` expands to fresh named source carrier
parameters `C0, ..., Cn`, each bounded by `ReadableTensor`, as defined under Core
Types. `ContractResult` is normalized before source type checking to one
`Tensor[U]`, where `U` is statically determined from
`C0::Elem, ..., Cn::Elem` by the contract multiply-and-sum promotion rules.
Neither `AnyTensor` nor `ContractResult` is a source-visible type.

Schema configuration methods:

```eglur-catalog
fn OpSchemaN.strict_names(self: Self) -> Self
fn OpSchemaN.relax_names(self: Self) -> Self
fn OpSchemaN.with_aliases(self: Self, map: Map[AxisName, AxisName]) -> Self
```

Rules:

1. `contract` semantics are fixed to `multiply` + `sum` in v0.1.
2. Axis grammar for `contract` MUST be atomic labels only.
3. `contract` patterns MUST reject `+`, `*`, numeric literals, grouped axis expressions, and axis-pack tokens (`...`, `...name`) with `CONTRACT_NON_ATOMIC_AXIS`.
4. Semiring customization (for example `.by(...)` or `.contract_by(...)`) — including any optional `.by` hooks hinted at in prior drafts — MUST NOT be exposed in v0.1 and MUST fail with `METHOD_NOT_FOUND`.
5. `contract` MUST accept one or more operands (arity `1..N`).
6. `contract` MUST accept exactly one right-hand-side axis list.
7. Axis lists on both sides MAY be empty; an empty axis list denotes rank-0.
8. Repeated labels are valid, including repeated labels inside one operand axis
   list. For each distinct label:
   1. labels MUST be validated in first-appearance order over the left-hand-side
      operand lists, and occurrences of one label MUST be visited in operand
      order and then source-axis order,
   2. the first occurrence supplies `expected_dim`; the first later unequal
      occurrence supplies `actual_dim` and MUST fail with
      `CONTRACT_REPEATED_LABEL_DIM_MISMATCH`,
   3. baseline broadcasting MUST NOT relax this equality, including dimensions
      `1` versus `N` or `1` versus `0`,
   4. for one operand of rank `r`, let the label occur at original source-axis
      positions `a0 < ... < a(k-1)`, where `k >= 2`; the operand MUST be
      normalized with
      `DIAGONAL(input, axes=[a0, ..., a(k-1)])`,
   5. one logical coordinate `q` selects source coordinate `q` at every listed
      axis; all unlisted source coordinates are preserved in source order,
   6. the retained diagonal axis occupies `a0`, later occurrences are removed,
      and the result rank is `r - (k - 1)`,
   7. a common extent of `0` produces an empty retained diagonal axis.
9. Several repeated-label groups in one operand MUST be normalized as if Rule 8
   were applied simultaneously against original source-axis positions.
10. Right-hand-side labels MUST be unique. The second occurrence of a bound
    output label MUST fail with `CONTRACT_OUTPUT_LABEL_DUPLICATE`.
11. Every right-hand-side label MUST belong to the union of all left-side
    labels (`CONTRACT_OUTPUT_LABEL_UNBOUND` otherwise). Validation scans the
    right-hand side left-to-right and checks whether each occurrence is bound
    before checking whether it duplicates an earlier occurrence.
12. Empty RHS MUST produce a rank-0 tensor output (`shape=()`), not an implicit scalar.
13. Lowering order MUST be deterministic:
   1. diagonal-normalize every operand under Rules 8-9,
   2. `out_labels`: output labels in RHS order,
   3. `red_labels`: unique labels not in outputs, in first-appearance order over LHS operands,
   4. `global_order = out_labels ++ red_labels`,
   5. each normalized operand is permuted to schema-label order, receives logical size-1 axes
      for labels absent from that operand, is expanded to `global_order`, then is
      multiplied elementwise and reduced over `red_labels`,
   6. every existing occurrence of one label remains subject to the exact-size
      constraint in Rule 8.
14. This deterministic lowering order defines logical alignment. Runtime
    primitive execution schedule remains implementation-defined under
    lowering-invariant determinism rules in `conventions.md`, except that
    fixed-width integer sum stages MUST remain exactly equivalent to the
    canonical checked reduction order defined there.
15. Contract multiply/sum dtype behavior MUST use the same `Promote(A,B)` rules as binary elementwise arithmetic:
   1. each fixed-width integer contribution is produced by the existing checked
      multiply path, then accumulated by the canonical checked left fold in the
      promoted result dtype,
   2. integer multiply or sum overflow MUST propagate as `Err(NumericError)`
      with cause `INT_OVERFLOW`.
16. Name-mode and alias configuration is schema-value-local:
   1. every fresh `contract{...}` schema MUST start in relaxed mode with an
      empty alias map,
   2. `strict_names`, `relax_names`, and `with_aliases` MUST return new schema
      values and MUST NOT mutate the receiver,
   3. configuration affects only the returned schema value and MUST NOT
      establish ambient state or propagate to a nested, sibling, or later fresh
      `contract{...}` expression.
17. `strict_names` sets the schema's name-checking mode to strict; in strict mode, incompatible axis-name bindings fail with `AXIS_MISMATCH`.
18. `relax_names` sets the schema's name-checking mode to relaxed; in relaxed mode, semantic name mismatches are not errors and implementations MAY drop conflicting output names as defined in `../axes.md`.
19. `with_aliases(map)` replaces the schema's alias map and preserves the current name-checking mode.
20. `with_aliases` alias-map direction is external input name to canonical schema label:
   1. the key is an accepted `AxisName` observed in input tensor overlays,
   2. the value is the canonical label from the `contract{...}` schema,
   3. for example, `.with_aliases(['row:'a, 'feature:'k, 'col:'b])` allows tensors named `'row`, `'feature`, and `'col` to satisfy `contract{ a k, k b -> a b }` in strict mode.
21. Alias canonicalization is one-step:
   1. before strict-name comparison, each observed input axis name `n` is canonicalized to `map.get(n).unwrap_or(n)`,
   2. alias values are not recursively looked up through the same map,
   3. names absent from the map keep their original name.
22. Alias validation rules:
   1. every alias target MUST be a label present in the left-hand-side label union of the schema,
   2. an alias key that is itself a schema label MUST map to itself; canonical schema labels MUST NOT be remapped,
   3. multiple external keys MAY map to the same canonical schema label,
   4. duplicate alias keys are ordinary duplicate-key map construction failures, not a contract-specific rule.
23. In strict mode, invalid alias configuration or strict-name failure after alias canonicalization MUST fail with `AXIS_MISMATCH`.
24. In relaxed mode, aliases have no effect on compatibility checking; relaxed-mode output-overlay rules still apply.
25. In strict mode, output axes corresponding to surviving output labels use canonical schema labels after alias canonicalization; accepted external aliases are not propagated as output names.
26. If `red_labels` has reduction volume `0`, the sum stage MUST produce the
    additive identity in the promoted `ContractResult` dtype for each existing
    output coordinate, as defined by `conventions.md`.


### `einop{...}` (`std::tensor::xform`)

`einop` is a composite tensor operation schema with fixed `multiply` + `sum` elimination semantics.

```eglur-catalog
schema einop{ <operands> -> <outputs> } -> EinopSchemaN
callable EinopSchemaN(inputs: AnyTensor...) -> Res[EinopResult, Union[EinopError, NumericError]]
```

For each call, `inputs: AnyTensor...` expands positionally to fresh named source
carrier parameters `C0, ..., Cn`, each bounded by `ReadableTensor`.
`EinopResult` is normalized before source type checking. A single-output schema
produces `Tensor[U]`; a schema with `N >= 2` outputs produces an `N`-tuple whose
element at index `i` has source type `Tensor[Ui]`. Exact tuple arity, order, and
element dtypes are determined by the schema and the static
`C0::Elem, ..., Cn::Elem` projections. Neither catalog meta type appears in the
resulting source signature.

Rules:

1. `einop` MAY be multi-input and multi-output.
2. The parser accepts the shared transform axis grammar. The v0.1 semantic
   subset for `einop` permits literals, variables, `*`, parentheses, and the
   shared axis-pack forms, but excludes `+`. Any `+` node on either side of the
   schema MUST fail with `AXIS_MAPPING_UNSUPPORTED` under the validation order
   in `../language/tensor-model.md`.
3. The authoritative identifier, literal, pack, diagonal, dtype, multi-output,
   error, and Core Tensor IR rules are the
   [`einop` normalization and lowering algorithm](../language/tensor-model.md#einop-normalization-and-lowering).
   This catalog section MUST NOT be interpreted as a second lowering contract.
4. Calling an `EinopSchemaN` requires exactly one tensor argument for each
   left-hand-side operand axis list. Every right-hand-side output independently
   consumes all of those original operands after the one shared schema solve.
5. `EinopResult` normalization MUST use only the schema and static
   `C0::Elem, ..., Cn::Elem` projections. Runtime dimensions, pack lengths, and
   empty domains MUST NOT affect source result types.
6. A single-output schema returns its materialized owning `Tensor[U0]` directly
   inside `Ok`.
7. A multi-output schema returns a tuple of materialized owning tensors inside
   `Ok`; tuple element order and element dtypes MUST match right-hand-side
   output order exactly. Multi-output results are observed through tuple
   projection (`out.0`, `out.1`, ...); v0.1 does not define stable tuple
   `to_str` formatting.
8. `einop` MUST NOT expose `reduce_by`/`contract_by` customization in v0.1;
   `.by` and related accessor references MUST fail with `METHOD_NOT_FOUND`.
9. Integer promotion conversion, multiply, or sum overflow in any output path
   MUST propagate as `Err(NumericError)` with cause `INT_OVERFLOW`. A
   multi-output call is expression-atomic and MUST NOT return partial
   successful outputs.
10. Fixed-width integer execution and empty reductions MUST follow the
    authoritative lowering plus the checked reduction and empty-domain
    contracts in `conventions.md`.


### Linalg Presets (`std::tensor::linalg`)

```eglur-catalog
value matmul: OpSchemaN
optional value bmm: OpSchemaN
```

`matmul` is the value of `contract{ a k, k b -> a b }`.
When provided, `bmm` is the value of `contract{ b a k, b k c -> b a c }`.


### Indexing as Gather/Scatter (`std::tensor::index`)

```eglur-catalog
union type IndexCoordinates = Tensor[i64] | TensorView[i64] | Seq[i64] | Vec[i64]
meta type SliceBound
sum type IndexItem = All | Slice(SliceBound, SliceBound, i64 = 1) | At(i64) | Take(IndexCoordinates) | NewAxis | Rest
alias type IndexSpec = Vec[IndexItem]
sum type ScatterMode = Assign | Add | Max | Min
```

Source code imports `IndexItem`, `IndexSpec`, and `ScatterMode` as needed, then
refers to sum variants as `IndexItem::Variant` and `ScatterMode::Variant`. The
short variant spellings above are catalog declaration syntax, not standalone
source imports.

`SliceBound` is the catalog-only constructor argument family containing exactly
`i64` values and `None`. It does not define a general implicit conversion from
`T` to `Opt[T]`; `IndexItem::Slice` stores the two accepted forms as its optional
start and stop bounds.
`RangeSpec` is the closed catalog subset of `IndexItem` containing
`IndexItem::All`, `IndexItem::Slice(...)`, and `IndexItem::Rest`.

Before coordinate or slice normalization, `slice`, `get_at`, `set_at`, and
`set_at!` MUST canonicalize their range/index vector against source rank `R`:

1. `IndexItem::All`, `IndexItem::Slice(...)`, `IndexItem::At(...)`, and
   `IndexItem::Take(...)` each consume one source axis.
2. `IndexItem::NewAxis` and `IndexItem::Rest` consume no source axis.
3. More than one `IndexItem::Rest` MUST fail with
   `INDEX_SPEC_REST_DUPLICATE` through `IndexError`. This check precedes the
   consuming-item arity check.
4. Let `K` be the number of consuming items written in the vector. `K > R`
   MUST fail with `INDEX_SPEC_ARITY_MISMATCH` through `IndexError`.
5. When one `IndexItem::Rest` is present, it is replaced in place by exactly
   `R - K` `IndexItem::All` items. A zero-width replacement is valid and simply
   removes `Rest`.
6. When no `IndexItem::Rest` is present, exactly `R - K` trailing
   `IndexItem::All` items are appended.
7. The canonical vector therefore contains no `Rest`, contains exactly `R`
   consuming items, and retains every written `NewAxis` in its original
   position. An empty vector canonicalizes to `R` `All` items.
8. All later shape, name-overlay, coordinate, duplicate-target, and update
   rules operate only on this canonical vector.

Const-evaluable failures use the same two codes in `compile_error`. Dynamic
failures return `Err(IndexError)` from the consuming operation.

Signed index normalization is defined per consumed axis of length `n` before bounds,
shape, broadcast, or duplicate-target validation:

1. For `IndexItem::At(i)` and every coordinate in `IndexItem::Take(...)`, let the normalized coordinate be
   `i` when `i >= 0`, and `n + i` otherwise. The normalized coordinate MUST be in
   the half-open interval `0 <= i < n`; otherwise the operation fails with
   `INDEX_OUT_OF_BOUNDS` through its
   `IndexError` channel.
2. For `IndexItem::Slice(start, stop, step)`, `step == 0` MUST fail with `SLICE_STEP_ZERO`
   through the operation's `IndexError` channel.
3. For positive `step`:
   1. omitted `start` defaults to `0` and omitted `stop` defaults to `n`,
   2. each explicit negative bound is first translated by adding `n`,
   3. translated bounds are clipped to the closed interval `0..=n`,
   4. selected coordinates are `start`, `start + step`, ... while the coordinate is
      less than `stop`.
4. For negative `step`:
   1. omitted `start` defaults to `n - 1` and omitted `stop` is the exclusive `-1`
      sentinel,
   2. each explicit negative bound is first translated by adding `n`,
   3. translated bounds are clipped to the closed interval `-1..=n - 1`,
   4. selected coordinates are `start`, `start + step`, ... while the coordinate is
      greater than `stop`.
5. An omitted negative-step `stop` and an explicit `stop = -1` are distinct: the
   omitted form retains the exclusive `-1` sentinel, while the explicit form is
   translated relative to `n` before clipping.
6. Slice bounds outside the axis are clipped and MUST NOT produce
   `INDEX_OUT_OF_BOUNDS`. A normalized slice MAY select zero coordinates.
7. The normalized coordinate sequence defines logical output order for gather and
   value-to-target correspondence for scatter.
8. `IndexItem::All` is equivalent to
   `IndexItem::Slice(None, None, 1)`.
9. Normalization and result-length calculation MUST use mathematical or sufficiently
   widened intermediate integers. In particular, `i64::MIN` bounds and steps MUST NOT
   overflow through negation, absolute-value, addition, or length calculation.
10. These rules apply identically after range/index canonicalization to
    `slice`, `get_at`, `set_at`, and `set_at!`.
11. Scatter duplicate-target validation occurs after all signed coordinates have been
    normalized, so aliases such as `-1` and `n - 1` denote the same target.
12. An empty normalized scatter target is a successful no-op when `values` can
    directed-expand to that target shape. Shape validation still occurs before
    that no-op.

```eglur-catalog
fn get_at[C: ReadableTensor](x: C, spec: IndexSpec) -> Res[Tensor[C::Elem], Union[IndexError, ShapeError]]
overload fn set_at[C: ReadableTensor](
  x: C,
  spec: IndexSpec,
  values: Tensor[C::Elem],
  mode: ScatterMode = ScatterMode::Assign,
  named profile: DeterminismProfile = DeterminismProfile::Default
)
  -> Res[Tensor[C::Elem], Union[IndexError, ShapeError, DTypeError, DeterminismError, NumericError]]
overload fn set_at[C: ReadableTensor](
  x: C,
  spec: IndexSpec,
  values: TensorView[C::Elem],
  mode: ScatterMode = ScatterMode::Assign,
  named profile: DeterminismProfile = DeterminismProfile::Default
)
  -> Res[Tensor[C::Elem], Union[IndexError, ShapeError, DTypeError, DeterminismError, NumericError]]
overload fn set_at![T](
  x: &mut Tensor[T],
  spec: IndexSpec,
  values: Tensor[T],
  mode: ScatterMode = ScatterMode::Assign,
  named profile: DeterminismProfile = DeterminismProfile::Default
)
  -> Res[Unit, Union[IndexError, ShapeError, DTypeError, DeterminismError, NumericError]]
overload fn set_at![T](
  x: &mut Tensor[T],
  spec: IndexSpec,
  values: TensorView[T],
  mode: ScatterMode = ScatterMode::Assign,
  named profile: DeterminismProfile = DeterminismProfile::Default
)
  -> Res[Unit, Union[IndexError, ShapeError, DTypeError, DeterminismError, NumericError]]
```

`get_at` is generalized gather and MUST be pure.

`set_at` is generalized scatter and MUST be pure:

1. it returns a tensor value with the requested scatter/update applied,
2. it MUST NOT mutate the input tensor value observable at the call site.

`set_at!` is generalized scatter and MUST be bang:

1. it updates the mutable receiver in place,
2. it MUST require a writable reference to the destination tensor, but that
   reference need not be unique under `calls-references.md`,
3. every reference alias to the destination observes the same committed
   result,
4. owning and view `values` arguments are evaluated as read-only retained
   values before update execution and do not become write-through aliases to
   the destination,
5. any `Err(...)` result MUST leave the receiver observationally unchanged
   through every alias.

The `set_at` overloads preserve the source carrier's `C::Elem` and accept
owning or view values of exactly that element type. The `set_at!` overloads keep
the destination concrete and mutable while accepting the same two read-only
value carriers. These overloads encode same-element constraints directly and
do not add associated-type equality reasoning.

`NumericError` in the `set_at` and `set_at!` signatures is a
dtype-conditional error member:

1. it is retained when the destination element dtype is a fixed-width integer
   dtype, because the runtime `mode` can select checked `ScatterMode::Add`;
2. it is omitted for `Bool`, floating, and complex destination dtypes;
3. specialization is based on the destination element dtype, not on the
   statically written or runtime value of `mode`;
4. unsupported mode/dtype pairs fail through `DTypeError`, not `NumericError`.

Indexing coordinate and gather rules:

1. `IndexItem::Take` accepts `Tensor[i64]`, `TensorView[i64]`, `Seq[i64]`, or
   `Vec[i64]` coordinate inputs.
2. `Seq[i64]` and `Vec[i64]` inputs to `IndexItem::Take` MUST be normalized to
   rank-1 `Tensor[i64]` coordinate tensors before gather/scatter validation.
3. Non-`i64` coordinate inputs to `IndexItem::Take` MUST fail with
   `CALL_ARG_TYPE_MISMATCH`. The signed `i64` coordinate dtype is intentional
   for tensor indexing and MUST NOT be replaced by `Nat` conversion at this API
   boundary.
4. All `Take` coordinate tensors in one canonical spec MUST broadcast directly
   to one common index shape `I` under the N-ary trailing-axis baseline algebra
   in `../language/tensor-model.md`. Incompatibility MUST fail with
   `BROADCAST_SHAPE_MISMATCH` through `ShapeError`. If the spec contains no
   `Take`, `I` does not exist and no common-index block is emitted.
5. The result shape is emitted by traversing the canonical spec from left to
   right while consuming source axes in order:
   1. `All` emits the consumed source-axis length;
   2. `Slice` emits its normalized selected-coordinate count;
   3. `At` emits no result axis;
   4. `NewAxis` emits one size-1 result axis without consuming a source axis;
   5. the first `Take` emits every dimension of `I`, and every later `Take`
      emits no result axis.
6. Rule 5 applies even when `Take` items are separated by basic items or
   `NewAxis`. The one common-index block always replaces the complete set of
   `Take` items at the first `Take` position; there is no adjacency-dependent
   reordering and the block is not moved unconditionally to the result front.
   Equivalently, result rank is the number of canonical `All` and `Slice` items,
   plus the number of `NewAxis` items, plus `rank(I)` when at least one `Take`
   is present.
7. A result coordinate maps to exactly one source coordinate as follows:
   1. each `All` or `Slice` result coordinate selects the corresponding
      normalized coordinate on its consumed source axis;
   2. each `At` contributes its one normalized fixed coordinate;
   3. the coordinate of `I` is mapped through each `Take` tensor's ordinary
      trailing-axis broadcast mapping, and that tensor value contributes the
      normalized coordinate for its consumed source axis;
   4. each `NewAxis` result coordinate is necessarily `0` and contributes no
      source coordinate.
8. Multiple `Take` inputs therefore behave as one zipped, broadcast coordinate
   tuple, not as a Cartesian product. Their logical traversal order is the
   ordinary row-major logical order of `I`, with the rightmost axis varying
   fastest.
9. Out-of-bounds `At` or `Take` coordinates after signed normalization MUST
   return `Err(IndexError)` and MUST emit `INDEX_OUT_OF_BOUNDS`; `Slice` bounds
   use the clipping rules above.
10. Gather axis-name overlays follow structural provenance after result-shape
    construction:
    1. `All` and `Slice` preserve the consumed source-axis name, if present;
    2. `At` removes the consumed source axis and its name;
    3. `NewAxis` emits an unnamed axis;
    4. when the canonical spec contains exactly one `Take` and `rank(I) == 1`,
       the one common-index axis preserves that consumed source-axis name, if
       present;
    5. when there are multiple `Take` items, or `rank(I) != 1`, every axis of
       the common-index block is unnamed.
11. Coordinate-tensor axis names never propagate to the result. Only the
    single-`Take`, rank-1 source-axis provenance exception above can name a
    common-index axis.
12. `set_at` and `set_at!` use the same canonical spec, coordinate mapping, and
    logical selection shape as `get_at`, but preserve the destination tensor's
    complete shape and name overlay.

Scatter value and update rules:

1. Let `target_shape` be the exact logical selection shape produced by the
   gather rules for the canonical spec. `values` MUST directed-expand to
   `target_shape` under `../language/tensor-model.md`; `target_shape` is fixed
   and MUST NOT grow through a symmetric common-shape calculation.
2. A statically decidable directed-expansion failure MUST emit
   `BROADCAST_SHAPE_MISMATCH` in `compile_error`. A dynamic failure MUST return
   `Err(ShapeError)` with the same code. The rule is identical for owning and
   view values and for `Assign`, `Add`, `Max`, and `Min`.
3. Axis-name overlays on `values` do not participate in expansion and do not
   affect the destination overlay.
4. Expanded values are read logically without requiring materialization. For
   each coordinate of `target_shape` in row-major logical order, the corresponding
   expanded value updates the source coordinate defined by the gather mapping.
5. If `target_shape` contains a zero-sized axis, directed-expansion validation
   still runs. After successful validation, scatter performs zero value reads
   and zero update combinations and returns success without changing the
   destination.
6. `set_at(..., mode=ScatterMode::Assign)` and
   `set_at!(..., mode=ScatterMode::Assign)` with duplicate target coordinates
   after signed normalization MUST fail with `SCATTER_SET_DUPLICATE_INDEX`.
7. Duplicate checks MAY be compile-time when indices are statically known. If
   static duplicate checks are not possible, runtime duplicate checks MUST be
   performed.
8. Scatter mode dtype domains are closed in v0.1:
   1. `Assign` accepts every supported tensor element dtype;
   2. `Add` accepts fixed-width integer, floating, and complex dtypes;
   3. `Max` and `Min` accept `Bool`, fixed-width integer, and floating dtypes;
   4. `Add` on `Bool`, and `Max` or `Min` on a complex dtype, MUST fail with
      `DTypeError` before update execution.
9. `set_at` and `set_at!` in `ScatterMode::Add`, `ScatterMode::Max`, or
   `ScatterMode::Min` mode MUST combine every logical update with the current
   candidate destination value according to the selected operator. Duplicate
   targets are repeated combinations in `target_shape` traversal order, not a
   pre-aggregated value.
10. `ScatterMode::Add` on a fixed-width integer destination dtype `T` MUST:
    1. begin with a candidate destination equal to the input destination;
    2. visit updates in increasing logical update traversal order;
    3. compute each candidate target as checked `T` addition of its current
       candidate value and the current update value;
    4. keep both the accumulator and each stored candidate value in exactly
       `T`, with no wider accumulator or final-only range check.
11. The first unrepresentable integer addition in Rule 10 MUST return
    `Err(NumericError)` with cause `INT_OVERFLOW`. A failed pure `set_at`
    expression yields no value and leaves its source unchanged. A failed
    `set_at!` MUST discard the complete candidate destination and leave its
    receiver observationally unchanged, including updates visited before the
    failure.
12. v0.1 provides no wrapping scatter mode. Modular update arithmetic requires
    an explicitly named future mode or API and MUST NOT be selected implicitly.
13. `profile` is a named argument and MUST be passed by name when provided.
14. `profile=DeterminismProfile::Default` requests the default profile for this
    call.
15. `profile=DeterminismProfile::Strict` requests strict primitive-level
    determinism for this call.
16. Determinism policy for scatter/update modes follows primitive-level profile
    contracts in `conventions.md`:
    1. every `default` execution MUST satisfy dtype-specific numeric equivalence
       + NaN/Inf classification constraints and MUST be bitwise-repeatable for
       identical compiled artifact, execution plan, execution context, and
       inputs;
    2. `strict` MUST be bitwise-equivalent to increasing logical index traversal
       baseline order.
17. If `profile=DeterminismProfile::Strict` is explicitly requested but
    unavailable on the active backend/device path, the operation MUST fail with
    `DETERMINISM_UNAVAILABLE`.
18. In v0.1, `set_at` and `set_at!` are the only required tensor APIs with a
    per-call `profile` parameter; tensor APIs without an explicit `profile`
    parameter execute under their active default determinism context.


### `map_axis` (`std::tensor::map_axis`)

Canonical API:

```eglur-catalog
transform map_axis(
  f: callback[pure],
  in_axes: InAxis | Vec[InAxis],
  out_axis: AxisIndex = axis(0),
  out_name: Opt[AxisName] = None
) -> callback[pure]
```

Type mapping requirements:

`map_axis` uses a closed, position-wise same-carrier contract in v0.1.

1. The returned pure callable MUST have exactly the same parameter list as `f`.
2. A mapped parameter of `f` MUST have the concrete type `Tensor[T]` or
   `TensorView[T]`.
3. Each mapped parameter preserves that concrete carrier independently:
   1. a `Tensor[T]` parameter makes the corresponding returned-callable
      parameter `Tensor[T]`, and each callback step receives a `Tensor[T]`;
   2. a `TensorView[T]` parameter makes the corresponding returned-callable
      parameter `TensorView[T]`, and each callback step receives a
      `TensorView[T]`.
4. One callback MAY mix `Tensor[...]` and `TensorView[...]` mapped positions.
5. An unmapped parameter preserves its exact source type and value.
6. These rules do not introduce an anonymous `ReadableTensor` parameter, a
   source union, an implicit carrier conversion, or a polymorphic function
   value. Generic named callbacks use the ordinary monomorphic expected-`Fn`
   instantiation rules in `../language/calls-references.md`.
7. For `map_axis(f, ...)`, an expected `Fn` type on the complete `map_axis`
   expression is the only transform-provided expected-`Fn` context. The
   compiler MUST combine `f`'s declared signature with that context to select
   one accepted result-family mapping and monomorphize `f` before constructing
   the returned callable.
8. If that expected type is absent or does not fully determine the generic type
   arguments, the function-value expression `f` MUST fail with
   `TYPE_ARG_INFERENCE_FAILED`. A later call through a binding that stores the
   returned callable MUST NOT propagate argument types backward to specialize
   `f`.
9. A returned-callable argument that is not assignable to the preserved
   parameter type MUST fail with `CALL_ARG_TYPE_MISMATCH`. In particular,
   `map_axis` MUST NOT adapt `Tensor[T]` to `TensorView[T]`, adapt
   `TensorView[T]` to `Tensor[T]`, or insert `materialize()`.

Let `R` be either one concrete `Tensor[U]` or one concrete `TensorView[U]`.
The three wrapper families below, each instantiated with one such `R`, are the
only callable result families accepted by `map_axis` in v0.1:

1. If `f` returns `R`, mapped call result MUST be
   `Res[Tensor[U], MapAxisError]`.
2. If `f` returns `Res[R, E]`, mapped call result MUST be
   `Res[Tensor[U], Union[E, MapAxisError]]`.
3. If `f` returns `Opt[R]`, mapped call result MUST be
   `Res[Opt[Tensor[U]], MapAxisError]`.

Normalization and validation:

Ordinary callable and effect validation precedes the tensor-specific rules
below. In particular, an effectful `f` fails with
`PURE_COMBINATOR_EFFECTFUL_CALLBACK` before an `in_axes`, `out_axis`, or
`out_name` failure in the same `map_axis` construction.

1. `in_axes` arity MUST match callable arity (`MAP_AXIS_IN_AXES_ARITY_MISMATCH` on mismatch).
2. At least one argument MUST be mapped (`MAP_AXIS_NO_MAPPED_ARGS` otherwise).
3. If an argument is neither `Tensor[...]` nor `TensorView[...]`, its
   corresponding `in_axes` MUST be `None` (`MAP_AXIS_NON_TENSOR_MAPPED`
   otherwise).
4. All mapped input axes MUST resolve and share the same `Nat` batch length `B` (`MAP_AXIS_BATCH_SIZE_MISMATCH` otherwise).
5. v0.1 does not define empty batch shape inference for `map_axis`; `B == 0` MUST fail with `MAP_AXIS_EMPTY_BATCH_UNSUPPORTED`.

Evaluation semantics:

1. Evaluation is observationally left-to-right over `i = 0..B-1`.
2. Implementations MAY parallelize but MUST preserve deterministic equivalence to left-to-right semantics.
3. For every mapped position, remove its mapped axis with `IndexItem::At(i)`
   semantics at each step.
4. A mapped `TensorView[T]` step MUST be a zero-copy affine view over the
   argument's retained base-buffer snapshot. It MUST preserve ordinary
   `TensorView` retention and mutation-isolation semantics.
5. A mapped `Tensor[T]` step MUST expose owning `Tensor[T]` value semantics.
   An implementation MAY avoid an eager per-step copy only when no callback can
   observe view semantics and all owning-`Tensor` mutation-isolation invariants
   remain satisfied.
6. For unmapped (`None`) arguments, pass unchanged values at each step.
7. Each callback invocation is an ordinary pure call. Scalar extraction and data-dependent pure control flow inside `f` remain valid.
8. Let `y0` be the first successful tensor or tensor-view result:
   1. Let `r = y0.rank()`.
   2. `out_axis.to_nat() <= r` MUST hold (`MAP_AXIS_OUT_AXIS_OUT_OF_RANGE` otherwise).
   3. `y0` defines the baseline shape for stack compatibility.
   4. `y0` defines the baseline `NameOverlay` for stack compatibility.
9. For each later successful result, shape equality MUST be validated before overlay equality.
10. A later shape mismatch MUST fail with `MAP_AXIS_RESULT_SHAPE_MISMATCH` at the earliest mismatching index.
11. After shape equality succeeds, a later result MUST have exactly the same `AxisIndex -> AxisName` bindings as the baseline overlay. `Map` insertion order is not semantically significant. The earliest mismatch MUST fail with `MAP_AXIS_RESULT_OVERLAY_MISMATCH`.
12. Every successful result already has the callback's single static element
    type `U` and concrete carrier `R`; v0.1 does not expose a recoverable
    runtime dtype- or carrier-mismatch path for a well-typed callback.
13. Successful results are stacked at `out_axis` into one materialized
    `Tensor[U]`.
14. A `TensorView[U]` callback result MUST be read directly into that stacked
    output. `map_axis` MUST NOT create a separate materialized `Tensor[U]` for
    each result merely to adapt its carrier.
15. If `out_name` is provided, inserted batch axis MUST receive `out_name`.
16. `out_name` MUST be rejected when it duplicates an existing result axis name (`MAP_AXIS_OUT_NAME_DUPLICATE`).

Fail-fast behavior:

1. For `f: ... -> Res[R, E]`, `map_axis(f)` MUST stop at first `Err`.
2. Diagnostics for that fail-fast MUST use `MAP_AXIS_RES_FAIL` and include the earliest failing 0-based index.
3. For `f: ... -> Opt[R]`, `map_axis(f)` MUST stop at first `None` and return `Ok(None)`.
4. Diagnostics for `None` short-circuit MUST use `MAP_AXIS_OPT_NONE` and include earliest 0-based index when emitted.


### Core Tensor IR Primitive Vocabulary (REQUIRED)

This vocabulary defines the minimum Core graph available when lowering complex
tensor operations. Source-observable AD support is the closed subset defined in
the v0.1 canonical AD-support graph; presence in this vocabulary alone does not
make a primitive differentiable.

Elementwise arithmetic/comparison/select:

```text
ADD, MUL, SUB, FDIV, IDIV, MAX, CMPLT, CMPEQ, WHERE, CAST, ISNAN
```

`CAST` carries a semantic `kind` of `numeric`, `wrapping`, or `bitwise` through
Core Tensor IR validation and AD-support classification. Source `astype`,
`wrapping_cast`, and `bitcast` lower with those respective kinds. A lowering or
optimization MUST NOT erase or merge the kind before every rule that depends on
conversion semantics has run. An implicit promotion between equal canonical
dtypes emits no `CAST`; an explicit source cast retains its Core identity even
when its source and destination dtypes are equal.

Recommended unary math primitives:

```text
EXP2, LOG2, SIN, SQRT, RECIPROCAL
```

Movement/view primitives:

```text
RESHAPE, PERMUTE, EXPAND, PAD, SHRINK, FLIP, DIAGONAL
```

`DIAGONAL(x, axes)` is a pure movement/view primitive with this contract:

1. `axes` MUST contain at least two distinct source-axis indices in increasing
   source order.
2. Every selected source axis MUST have the same extent.
3. One output coordinate `q` maps to source coordinate `q` at every selected
   axis. Unselected source coordinates retain their source order.
4. The first selected axis position is retained, later selected positions are
   removed, and output rank is `rank(x) - (axes.len() - 1)`.
5. A selected extent of `0` produces an empty retained axis.
6. `DIAGONAL` MUST NOT introduce `REALIZE` in Core Tensor IR. For an affine
   input layout, its retained-axis stride is the sum of the selected source
   strides, its base offset is unchanged, and unselected strides are preserved.
7. A backend MAY erase `DIAGONAL` into affine layout metadata or decompose it
   into other movement primitives, provided the logical coordinate mapping and
   first-position retention above remain unchanged.

Reduction primitives:

```text
REDUCE_AXIS_SUM, REDUCE_AXIS_MAX
```

`REDUCE_AXIS_SUM(x, ordered_axes)` is one logical multi-axis reduction node:

1. `ordered_axes` MUST contain distinct valid source-axis indices. Its order is
   semantic contributor order, not merely an unordered axis set.
2. Unselected axes retain their source order. Output coordinates are visited in
   logical row-major order over those retained axes.
3. For one output coordinate, contributor coordinates are visited in logical
   row-major order over `ordered_axes`, with the last listed axis varying
   fastest.
4. The node removes all listed axes simultaneously. A lowering to several
   single-axis reductions is permitted only when it preserves the same
   contributor sequence, dtype, value, and failure behavior.
5. Fixed-width integer execution MUST use the checked left fold in the input
   element dtype from `conventions.md`; it MUST NOT reassociate or accumulate in
   a wider dtype. Empty contributor domains use the operation-selected additive
   identity contract.
6. `contract` and `einop` MUST supply their normative `red_labels` or
   `red_keys` sequence as `ordered_axes`. Primitive and default reductions MUST
   supply the operation-specific canonical axis order from `conventions.md`.

Indexing primitives:

```text
IOTA, GATHER, SCATTER_SET, SCATTER_ADD, SCATTER_MAX, SCATTER_MIN
```

Materialization barrier:

```text
REALIZE
```

Kernel/codegen uops (for example `LOAD`, `STORE`, `INDEX`, `RANGE`, `IF`, `BARRIER`) are not part of this Core Tensor IR contract and are outside AD transform domain.


### `grad` / `value_and_grad` (`std::tensor::grad`)

Canonical APIs:

```eglur-catalog
transform grad(
  f: callback[pure],
  argnum: Nat = 0
) -> callback[pure]

transform value_and_grad(
  f: callback[pure],
  argnum: Nat = 0
) -> callback[pure]
```

Type mapping requirements:

Let the differentiated argument selected by `argnum` have concrete type
`Tensor[D]` or `TensorView[D]`, and let the primal scalar output have type
`Tensor[U]`.
The three result families below are the only callable result families accepted by `grad` and `value_and_grad` in v0.1.
Each returned pure callable has the same parameter list as `f`.
In particular, the differentiated parameter and every other parameter preserve
their exact concrete source types in the returned callable.

1. If `f` returns `Tensor[U]` with scalar rank (`rank == 0`):
   1. `grad(f)` call result MUST be `Res[Tensor[D], GradError]`.
   2. `value_and_grad(f)` call result MUST be `Res[(Tensor[U], Tensor[D]), GradError]`.
2. If `f` returns `Res[Tensor[U], E]` with scalar rank:
   1. `grad(f)` call result MUST be `Res[Tensor[D], Union[E, GradError]]`.
   2. `value_and_grad(f)` call result MUST be `Res[(Tensor[U], Tensor[D]), Union[E, GradError]]`.
3. If `f` returns `Opt[Tensor[U]]` with scalar rank:
   1. `grad(f)` call result MUST be `Res[Opt[Tensor[D]], GradError]`.
   2. `value_and_grad(f)` call result MUST be `Res[Opt[(Tensor[U], Tensor[D])], GradError]`.

The gradient carrier is always owning `Tensor[D]`, including when the
differentiated argument is `TensorView[D]`. In that case, differentiation is
with respect to the supplied view's logical values and shape, not with respect
to the retained source tensor or base buffer from which the view was derived.
No source/base-shaped cotangent is returned implicitly.

Validation and execution rules:

Ordinary callable and effect validation precedes the tensor-specific rules
below. An effectful `f` fails with `PURE_COMBINATOR_EFFECTFUL_CALLBACK` before
`argnum` or differentiated-argument validation in the same `grad` or
`value_and_grad` construction.

1. `argnum` MUST be within callable arity (`GRAD_ARGNUM_OUT_OF_RANGE` otherwise).
2. Differentiated argument MUST be tensor (`GRAD_NON_TENSOR_ARG` otherwise).
3. Differentiated argument dtype MUST be floating (`GRAD_NON_FLOAT_DTYPE` otherwise).
4. Scalar-gradient mode requires primal output rank 0 (`GRAD_PRIMAL_NOT_SCALAR` otherwise).
5. `grad` and `value_and_grad` MUST lower to Core Tensor IR before AD.
6. Lowering failure MUST return `GRAD_LOWERING_FAILED`.
7. Unsupported primitive in differentiable path MUST return `GRAD_UNSUPPORTED_PRIMITIVE`.
8. For `f: ... -> Res[Tensor[U], E]`, `Err(e)` MUST short-circuit and return `Err(e)` without AD execution.
9. For `f: ... -> Opt[Tensor[U]]`, `None` MUST short-circuit and return `Ok(None)` without AD execution.
10. Gradient shape MUST match the differentiated argument's logical shape.
11. Gradient dtype MUST equal differentiated argument dtype.
12. Implementations MAY use higher precision internally, but returned gradient MUST be cast to primal differentiated argument dtype.
13. `MAX` and `REDUCE_AXIS_MAX` VJP tie policy MUST be deterministic and uniform:
   1. For binary `MAX`, the contributor set is the two aligned input values at
      one output coordinate. For `REDUCE_AXIS_MAX`, it is the ordered reduction
      slice for one output coordinate.
   2. When no contributor is NaN, the tie set contains every contributor equal
      to the forward maximum. When any contributor is NaN, the tie set contains
      exactly the NaN contributors.
   3. If the incoming cotangent is `dy` and the tie-set cardinality is the
      positive mathematical integer `k`, each tied contributor receives the
      mathematical quotient `dy / k` and every other contributor receives zero,
      followed by the final dtype conversion permitted by Rule 12.
   4. Trailing-axis baseline broadcasting from
      `../language/tensor-model.md` applies to the local cotangent values.
   5. This rule fixes observable values, not a required internal mask, count,
      saved-value, or lowering program.
14. Forward `MAX` and `REDUCE_AXIS_MAX` semantics in v0.1 are NaN-propagating.
15. Acceptance of a concrete tensor parameter does not prove differentiability.
    The complete differentiable path MUST still satisfy the Core Tensor IR
    lowering and the closed
    [v0.1 canonical AD-support graph](../language/tensor-model.md#v01-canonical-ad-support-graph).
16. Every well-typed callback admitted by that graph MUST succeed on a
    conforming implementation. A backend-specific lowering choice MUST NOT turn
    an admitted graph into `GRAD_LOWERING_FAILED` or
    `GRAD_UNSUPPORTED_PRIMITIVE`.
17. Reverse-only permission does not imply source-active permission. In
    particular, generated `SCATTER_ADD` and numeric `CAST` nodes do not make a
    source-active scatter or cast differentiable in v0.1.
18. `map_axis(grad(f))` applies an already-formed gradient callable independently
    to slices; it does not differentiate `map_axis` or require primitive
    batching. A nested `grad(grad(f))` or equivalent higher-order transform is
    unsupported in v0.1.
