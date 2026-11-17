# `std::prelude` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Core Types

The prelude MUST provide:

1. `Unit`.
2. `Never`.
3. `Bool`.
4. `Tri` (three-valued logic type with variants `Tri::True`, `Tri::False`, `Tri::Unknown`).
5. Kind-level numeric umbrellas: `Int`, `UInt`, `Integer`, `Float`, `Complex`.
6. Domain scalar types: `Nat`, `PositiveNat`.
7. Fixed-width signed integers: `i8`, `i16`, `i32`, `i64`.
8. Fixed-width unsigned integers: `u8`, `u16`, `u32`, `u64`.
9. Floating types: `f16`, `bf16`, `f32`, `f64`.
10. Complex types: `complex64`, `complex128`.
11. `Str`.
12. `Path`.
13. `Seq[T]`, `Vec[T]`, `Map[K,V]`, `Set[T]`.
14. `Opt[T]` and `Res[T,E]`.
15. `Box[T]` intrinsic layout-indirection type.
16. `Own[T]` intrinsic re-export when ownership surface is enabled.
17. `Ordering` with exactly the variants `Less`, `Equal`, and `Greater`.
18. `ScopedFailure[E]` for scoped-resource acquisition, callback, and close
    failures.

Primitive type names (`Unit`, `Bool`, `Str`, `Nat`, `PositiveNat`, and the
fixed-width numeric types) are ordinary type-namespace prelude bindings, not
reserved spellings. Module declarations, explicit imports, and in-scope
generic parameters MAY shadow them under `../language/modules-resolution.md`.
An explicit `std::prelude::<name>` path or an import alias from that module
MUST denote the canonical primitive type. Aliases and re-exports preserve that
identity; they do not introduce nominal types.

Literal types are independent of these bindings: `()`, Boolean literals, and
numeric literal defaults retain their canonical types when a same-spelled
source name is shadowed. Expected-type inference follows the resolved type,
not its spelling. The reserved intrinsic names listed in
`../language/algebraic-data.md` remain reserved.

`Unit` is the language-intrinsic singleton type. The prelude provides the type
binding only and MUST NOT provide a value binding named `Unit`; the sole
built-in Unit value is written `()` as specified by
`../language/type-system.md`.

`Never` is the same language-intrinsic bottom type defined in
`../language/type-system.md`. The prelude provides only that type binding: it
adds no value binding, constructor, duplicate nominal declaration, or automatic
trait implementation.

Catalog-owned prelude types and type re-exports:

```eglur-catalog
sum type Tri = True | False | Unknown
sum type Ordering = Less | Equal | Greater
sum type ScopedFailure[E] = Primary(E) | PrimaryAndClose { primary: E, close: E }
reexport type Path = std::io::Path
reexport type Seq = std::seq::Seq
reexport type Vec = std::vec::Vec
reexport type Map = std::map::Map
reexport type Set = std::set::Set
```

`ScopedFailure[E]` is the shared error carrier for standard-library APIs that
own a resource across a scoped callback. Its variants do not expose the phase
of a single failure. The construction and precedence rules are defined once in
`conventions.md`. Library-owned convenience operations do not acquire this
carrier solely because their implementation has multiple lifecycle phases.

Shared baseline error types:

```eglur-catalog
opaque type IndexError
opaque type DuplicateKeyError
opaque type DomainError
opaque type NumericError
```

Rules:

1. `IndexError` is the nominal shared error type for baseline index and bounds failures in `std::vec` and `std::tensor`.
2. `DuplicateKeyError` is the nominal shared error type for duplicate-key conversions in `std::seq` and `std::map`.
3. `DomainError` is the nominal shared error type for explicit runtime conversion into domain scalar types such as `Nat` and `PositiveNat`.
4. `NumericError` is the nominal shared error type for checked fixed-width scalar and tensor arithmetic faults.
5. Modules MAY re-export these names for local ergonomics, but MUST NOT define distinct module-local nominal error types with the same names.

Prelude trait surface MUST include:

1. `Display`
2. `Debug`
3. `Clone`
4. `Eq`
5. `Hash`
6. `Ord`
7. `Default`
8. `From`
9. `TryFrom`
10. `Error`
11. `ReadableTensor` (re-exported from `std::tensor`).

Catalog-owned prelude trait re-export:

```eglur-catalog
reexport trait ReadableTensor = std::tensor::ReadableTensor
```

Rules:

1. The list above defines visible prelude trait symbols, not the built-in derive provider set.
2. Built-in derive support is intentionally narrower and is specified by `../language/algebraic-data.md`.
3. A prelude trait with no active derive provider MUST still fail under `#[derive(...)]` with `DERIVE_TRAIT_UNSUPPORTED`.
4. The `ReadableTensor` re-export preserves
   `std::tensor::ReadableTensor` identity and makes its methods available to
   ordinary method lookup without an explicit trait import.
5. The trait-symbol re-export does not import tensor types, constructors,
   transforms, dtype descriptor values, or a tensor backend into user modules.
   Programs that do not use a tensor surface acquire no tensor runtime
   requirement from this name alone. The complete activation and implementation
   boundary is defined in `../language/implementation-boundary.md`.

Required source-callable methods owned by `std::prelude` traits:

```eglur-catalog
fn Clone.clone(self: &Self) -> Self
fn Display.to_str(self: &Self) -> Str
fn Eq.eq(self: &Self, other: &Self) -> Bool
fn Hash.hash(self: &Self, seed: u64) -> u64
fn Ord.cmp(self: &Self, other: &Self) -> Ordering
```

Rules:

1. `Clone.clone` MUST be pure and MUST NOT consume or mutate the receiver.
2. `Display.to_str` MUST be pure and total and MUST NOT consume or mutate the receiver.
3. `Display.to_str` has no default implementation. Each applicable `Display` impl MUST provide that exact method signature.
4. `Eq.eq` MUST be pure and total and MUST NOT consume or mutate either operand.
5. `Eq.eq` has no default implementation. Each applicable `Eq` impl MUST provide that exact method signature.
6. Every `Eq` implementation MUST define an equivalence relation: reflexive, symmetric, and transitive.
7. The compiler is not required to prove Rule 6 for user-written impls and MUST NOT rely on those laws for memory safety.
8. `PartialEq` is not a v0.1 prelude symbol or comparison capability.
9. `Hash.hash` MUST be pure and total and MUST NOT consume or mutate the receiver.
10. `Hash.hash` has no default implementation. Each applicable `Hash` impl MUST provide that exact method signature.
11. `seed` is the incoming hash accumulator. An implementation MUST compute its result only from the receiver and that accumulator and MUST NOT consult hidden time, randomness, process-local state, or a global seed. A singleton value with no payload MAY return the supplied seed unchanged.
12. Within one hash determinism context, identical receiver values and identical seeds MUST produce the same `u64` across repeated calls and separate executions. A hash determinism context is fixed by the Eglur implementation identity and version together with the target platform.
13. Exact hash outputs are not portable across hash determinism contexts and are not a stable ABI, serialization, cache-file, or network format. Conformance tests MUST NOT require an exact numeric hash output.
14. `Hash` and `Eq` are independent capabilities. A type MAY implement either without implementing the other.
15. When a type implements both `Eq` and `Hash`, `Eq.eq(&a, &b) == true` MUST imply `Hash.hash(&a, seed) == Hash.hash(&b, seed)` for every `seed`.
16. Unequal values MAY have equal hashes. Hash quality affects performance and resilience, not value identity.
17. The compiler is not required to prove Rules 12, 15, or 16 for user-written impls and MUST NOT rely on those laws for memory safety.
18. `Ord.cmp` MUST be pure and total and MUST NOT consume or mutate either operand.
19. `Ord.cmp` has no default implementation. Each applicable `Ord` impl MUST provide that exact method signature.
20. Every `Ord` implementation MUST define a total order coherent with `Eq`:
    1. exactly one of `Ordering::Less`, `Ordering::Equal`, or `Ordering::Greater` is returned for each operand pair,
    2. `cmp(a, b) == Ordering::Equal` iff `Eq.eq(a, b)` is true,
    3. reversing operands reverses `Ordering::Less` and `Ordering::Greater` and preserves `Ordering::Equal`,
    4. the induced less-than relation is transitive.
21. The compiler is not required to prove Rule 20 for user-written impls and MUST NOT rely on those laws for memory safety.
22. `PartialOrd` is not a v0.1 prelude symbol or comparison capability.
23. Other prelude trait names MAY be consumed by operators, formatting, derive eligibility, or standard-library contracts without exposing additional source-callable trait methods in v0.1.

Required v0.1 `Eq` implementations:

1. `Unit`, `Bool`, `Tri`, `Ordering`, `Str`, every concrete numeric scalar type, `Nat`, and `PositiveNat` MUST implement `Eq`.
2. A tuple implements `Eq` iff every element type implements `Eq`. Tuple equality compares elements in positional order and short-circuits at the first unequal pair.
3. `Opt`, `Res`, `Seq`, and `Vec` provide conditional implementations defined by their owning module specifications.
4. Standard-library nominal types MAY provide required `Eq` implementations only where their defining module specifies the equality identity.
5. No other nominal or generic type implements `Eq` merely because both operands have the same static type.
6. User-defined structs and enums implement `Eq` only through an explicit impl or the built-in `Eq` derive from `../language/algebraic-data.md`.

Required v0.1 `Hash` implementations:

1. `Unit`, `Bool`, `Tri`, `Ordering`, `Str`, every concrete numeric scalar type, `Nat`, and `PositiveNat` MUST implement `Hash`.
2. Floating and complex implementations MUST hash the same component `total_key` values used by the unified equality policy in `conventions.md`.
3. A tuple implements `Hash` iff every element type implements `Hash`. Tuple hashing starts with the caller-supplied seed and hashes elements in positional order, passing each result as the next element's seed.
4. `Opt`, `Res`, `Seq`, and `Vec` provide conditional implementations defined by their owning module specifications.
5. Standard-library nominal types MAY provide required `Hash` implementations only where their defining module specifies the hashed identity.
6. An `Eq` implementation does not automatically provide `Hash`, and a `Hash` implementation does not automatically provide `Eq`.
7. User-defined structs and enums implement `Hash` only through an explicit impl or the built-in `Hash` derive from `../language/algebraic-data.md`.
8. `Map`, `Set`, `Box`, `Own`, `Task`, references, and callable types do not implement `Hash` in v0.1.

Required v0.1 `Ord` implementations:

1. Fixed-width signed and unsigned integers, floating scalar types, `Nat`, and `PositiveNat` MUST implement `Ord`.
2. Floating implementations MUST use the unified total-order key from `conventions.md`.
3. `complex64` and `complex128` do not implement `Ord`.
4. `Unit`, `Bool`, `Tri`, `Ordering`, `Str`, tuples, ordinary carriers, and other standard-library nominal types do not implement `Ord` unless their defining module explicitly says otherwise.
5. User-defined structs and enums implement `Ord` only through an explicit impl or the built-in `Ord` derive from `../language/algebraic-data.md`.
6. Every `Ord` implementation target MUST implement `Eq`. The compiler-known prerequisite `std::prelude::Ord -> std::prelude::Eq` is available to generic bound checking.

Numeric umbrellas (`Int`, `UInt`, `Integer`, `Float`, `Complex`) are abstract kinds, not concrete value dtypes.

`Nat` and `PositiveNat` are concrete domain scalar types. Their typing, range, literal, comparison, and conversion rules are defined in `../language/type-system.md`.

`Own[T]` is not a numeric kind; it is ownership metadata surface from the language core and MUST NOT alter effect authorization rules.

`Own[T]` prelude contract:

1. `Own[T]` is language-intrinsic in ownership-enabled profiles.
2. `std::prelude` MUST re-export that same intrinsic symbol as `Own`.
3. Implementations MUST NOT model prelude `Own` as a distinct nominal stdlib type.
4. `Own[T]` does not forward methods or trait implementations from `T` and implements neither `Clone`, `Eq`, nor `Hash` in v0.1.
5. Standard-library modules MAY declare methods directly on designated `Own[R]` resource instantiations; these are wrapper methods, not forwarded methods from `R`.

`Box[T]` prelude contract:

```eglur-catalog
fn Box.new[T](value: T) -> Box[T]
fn Box[T].get(self: &Self) -> T
```

1. `Box[T]` is the language-intrinsic, ownership-free indirection boundary defined by `../language/algebraic-data.md`.
2. `T` MUST be ownership-free and reference-free; `Box[Own[T]]` and every other ownership-bearing instantiation fail with `OWNERSHIP_TYPE_POSITION_FORBIDDEN`, while reference-bearing arguments fail with `REFERENCE_TYPE_POSITION_FORBIDDEN`.
3. `Box.new` and `Box.get` are pure.
4. `Box.new` stores a value observationally independent from its argument after construction.
5. `Box.get` returns the contained value under ordinary value semantics and does not mutate or consume the box.
6. Implementations MAY share storage or use copy-on-write, but mutation of the source, returned value, or another box MUST NOT expose aliasing through `Box`.
7. `Box` has no baseline bang mutation API, raw-address API, or ownership-discharge behavior.
8. `Box[T]` does not implement `Eq` in v0.1, even when `T` implements `Eq`.

Scalar numeric arithmetic and tensor numeric arithmetic MUST share one promotion lattice, defined in `conventions.md`.

Scalar wrapping arithmetic surface:

```eglur-catalog
meta type FixedWidthInteger[T]
fn wrapping_add[T](lhs: T, rhs: T) -> T requires FixedWidthInteger[T]
fn wrapping_sub[T](lhs: T, rhs: T) -> T requires FixedWidthInteger[T]
fn wrapping_mul[T](lhs: T, rhs: T) -> T requires FixedWidthInteger[T]
```

`FixedWidthInteger[T]` holds exactly for `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, and `u64`.

Rules:

1. These functions are source-callable prelude functions and require no explicit import.
2. Each function requires both operands to have the same concrete fixed-width integer dtype and returns that dtype.
3. Evaluation uses two's-complement modulo `2^bits(T)` semantics and MUST NOT emit `INT_OVERFLOW`.
4. Floating, complex, abstract numeric-kind, domain-scalar, and mixed-dtype arguments are not accepted by this overload family.
5. These functions are the scalar escape hatch for intentional modular arithmetic outside `checked {}`.
6. Bare integer literal operands alone do not select a narrow
   `FixedWidthInteger[T]` instantiation. Without an established expected type,
   `wrapping_mul(64, 2)` infers `T = i64`; narrow modular behavior requires an
   expected result type such as `let x: i8 = wrapping_mul(64, 2)` or explicit
   call-site type arguments such as `wrapping_mul::[i8](64, 2)`.

Domain scalar conversion surface:

```eglur-catalog
fn Nat.from_i64(n: i64) -> Res[Nat, DomainError]
fn PositiveNat.from_i64(n: i64) -> Res[PositiveNat, DomainError]
fn Nat.to_i64(self: Nat) -> i64
fn PositiveNat.to_nat(self: PositiveNat) -> Nat
```

Rules:

1. Conversion failure MUST return `Err(DomainError)` and emit diagnostic cause `DOMAIN_INVALID_VALUE`.
2. `Nat.to_i64` is total.
3. `PositiveNat.to_nat` is total and preserves the mathematical value.


### Formatting

`Display.to_str` is the canonical source-callable display contract. The prelude free function is its functional adapter:

```eglur-catalog
fn to_str[T](value: T) -> Str requires Display[T]
```

Rules:

1. `to_str(value)` MUST resolve through the unique `Display` implementation for the static type of `value` and MUST produce the same `Str` as the fully qualified call `Display.to_str(&value)`.
2. The free adapter evaluates `value` exactly once and is source-callable without an explicit import.
3. Method syntax follows the ordinary lookup order from `../language/modules-resolution.md`: absent an applicable inherent `to_str`, `value.to_str()` selects the visible `Display.to_str` trait method; an inherent method with that name takes precedence, while `to_str(value)` and `Display.to_str(&value)` continue to select the trait implementation.
4. Required standard-library `Display` implementations MUST NOT declare a distinct inherent `to_str` method, so method syntax and the free adapter are equivalent for those types.
5. A user-defined `impl Display for LocalType` that satisfies ordinary coherence rules supplies the trait-method candidate and enables `to_str(value)`; no separate free-function overload is declared or merged.
6. Required unconditional `Display` implementations are `Bool`, every fixed-width signed and unsigned integer type, `Nat`, `PositiveNat`, `f32`, `f64`, and `Str`.
7. Required conditional implementations are:
   1. `Display[Opt[T]]` when `Display[T]` holds,
   2. `Display[Res[T,E]]` when both `Display[T]` and `Display[E]` hold,
   3. `Display[Seq[T]]` and `Display[Vec[T]]` when `Display[T]` holds,
   4. `Display[ScopedFailure[E]]` when `Display[E]` holds.
8. Tensor `DType[T]` implementations and their canonical tokens are normatively specified in `tensor.md`; `std::prelude` does not import or enumerate tensor descriptor types.
9. Operational error and error-kind `Display` implementations and their exact
   strings are normatively specified by `io.md`, `net.md`, and `time.md`.
10. Implementations MAY provide `Display` for additional scalar or container types, but required v0.1 conformance MUST NOT depend on those additions unless their formatting is normatively fixed elsewhere.
11. `f16`, `bf16`, complex scalar types, and `Tri` have no required `Display` implementation in v0.1.

Formatting used by required diagnostics and conformance snapshots MUST be stable and deterministic for the following value families:

1. `Bool`:
   1. `to_str(true) == "true"`,
   2. `to_str(false) == "false"`.
2. Signed and unsigned integer scalars:
   1. base-10 digits only,
   2. no `_` separators,
   3. `-` appears only for negative signed values.
3. `Nat` and `PositiveNat`:
   1. base-10 digits only,
   2. no `_` separators,
   3. no sign prefix.
4. Floating scalars (`f32`, `f64`) use canonical float display formatting:
   1. finite values are formatted with the shortest decimal significand that round-trips to the same value and dtype through the matching baseline parser (`parse_f32` for `f32`, `parse_f64` for `f64`),
   2. positive finite zero formats as `0.0`,
   3. negative finite zero formats as `-0.0`,
   4. fixed notation MUST be used for finite nonzero values with `1e-6 <= abs(x) < 1e21`,
   5. exponent notation MUST be used for finite nonzero values outside that range,
   6. exponent notation MUST use lowercase `e`, omit `+` for positive exponents, and omit leading zeros in the exponent digits,
   7. insignificant trailing fractional zeros MUST NOT be emitted, except for the required `.0` cue below,
   8. a trailing decimal point MUST NOT be emitted,
   9. when a finite result would otherwise contain neither `.` nor `e`, `.0` MUST be appended so the value remains visibly floating-point,
   10. positive infinity formats as `inf`,
   11. negative infinity formats as `-inf`,
   12. all NaN values format as `nan`; v0.1 `to_str` does not expose NaN sign or payload.
5. `Str`:
   1. `to_str(s) == s`.
   2. When a `Str` value is formatted as an element inside a stable container snapshot, it MUST use double-quoted string-literal rendering with the required escapes from `language/syntax.md`.
6. `Opt[T]` when `Display[T]` holds:
   1. `None` formats as `None`,
   2. `Some(x)` formats as `Some(` + formatted `x` + `)`.
7. `Res[T,E]` when both `Display[T]` and `Display[E]` hold:
   1. `Ok(x)` formats as `Ok(` + formatted `x` + `)`,
   2. `Err(e)` formats as `Err(` + formatted `e` + `)`.
8. `Seq[T]` and `Vec[T]` when `Display[T]` holds:
   1. values format as `[e0,e1,...,en]`,
   2. element order is preserved exactly,
   3. no spaces are inserted around delimiters,
   4. `Str` elements use the stable container-element rendering rule above, not raw top-level `to_str(Str)` output.
9. `ScopedFailure[E]` when `Display[E]` holds:
   1. `ScopedFailure::Primary(error)` has exactly the same display as `error`,
   2. `ScopedFailure::PrimaryAndClose { primary, close }` formats as
      `concat(["scoped body failed: ", primary.to_str(), "; close also failed: ", close.to_str()])`.

Tuple values do not have a required `Display` implementation in v0.1.
Required conformance MUST project tuple elements (for example `t.0`, `t.1`) or otherwise convert them to stable scalar/container values before formatting snapshots.

Implementations MAY additionally provide `Display` for `Map[K,V]`, `Set[T]`, and other container families, but REQUIRED v0.1 conformance MUST NOT depend on those additional formatting contracts unless the relevant type's formatting is normatively fixed elsewhere.

String composition and formatting surface:

```eglur-catalog
reexport type FormatError = std::str::FormatError
reexport fn concat = std::str::concat
reexport fn format = std::str::format
```

This surface is defined normatively in `std::str`; `std::prelude` re-exports it for convenience.

Global string-usage rules in v0.1:

1. `+` is not string concatenation in v0.1.
2. String composition MUST use `concat` or formatting forms.
3. Interpolated string literals `f"..."` use the same placeholder grammar (`{name}`, `{name:?}`) and are compile-time checked:
   1. unresolved names are compile-time errors,
   2. formatting-trait availability is compile-time checked,
   3. successful `f"..."` evaluation produces `Str` without runtime `Res` wrapping.


### Generic Tap Operators (REQUIRED)

```eglur-catalog
fn T.tap[T](self: T, f: callback[pure](&T) -> Unit) -> T
fn T.tap![T](self: T, f: callback[effectful](&T) -> Unit) -> T
```

Semantics:

1. `tap` MUST call `f` exactly once and MUST return original `self` unchanged.
2. `tap` callback `f` MUST be pure.
3. `tap!` MUST call `f` exactly once and MUST return original `self` unchanged.
4. `tap!` callback MAY perform effects.
5. The generic `T` in these signatures is ownership-free and reference-free in v0.1; `tap`/`tap!` are not ownership-transfer or hidden-borrow combinators.
