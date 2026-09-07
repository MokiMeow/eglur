# Cast Legality and Value Domain (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

This section defines normative cast legality states and value-domain validity
rules for the scalar dtype identities that inhabit the required tensor
`DType[T]` family.

Tensor `astype` MUST follow this section 1:1 for source/destination tensor
dtypes listed in `tensor.md`. The `IMPLICIT_ALLOWED` state also constrains the
closed scalar and tensor promotion contexts listed below.

v0.1 has no general explicit scalar-cast API. Such an API, including explicit
scalar `Tri` conversions, is deferred under `STDLIB-OI-025`; this matrix does
not reserve source syntax or a callable shape for it. Scalar `Bool` to `Tri`
lifting in logical operations is a separate baseline typing rule and does not
admit `Tri` to the cast matrix.


### Legality States

For a source/destination dtype pair, exactly one legality state applies:

1. `FORBIDDEN`
2. `EXPLICIT_ONLY`
3. `EXPLICIT_LOSSY`
4. `IMPLICIT_ALLOWED`

State meanings:

1. `FORBIDDEN`: pair is illegal; cast MUST fail with `CAST_INVALID_DTYPE_PAIR`.
2. `EXPLICIT_ONLY`: explicit cast is allowed; implicit conversion is disallowed.
3. `EXPLICIT_LOSSY`: explicit cast is allowed and MAY lose information for some values; implicit conversion is disallowed.
4. `IMPLICIT_ALLOWED`: explicit cast is allowed and implicit conversion MAY occur only in contexts listed below.


### Implicit Conversion Scope

Implicit conversion is NOT global. It is limited to:

1. scalar numeric operator/promotion contexts defined in `conventions.md`,
2. tensor numeric operator and primitive promotion contexts defined in
   `conventions.md`.

Implicit conversion is disallowed for:

1. assignment,
2. argument passing,
3. return expression typing,
4. struct field initialization,
5. pattern matching.


### Type Universe for Matrix Axes

The legality matrix is defined on concrete scalar dtype identities that have a
required tensor `DType[T]` witness:

1. `Bool`
2. `i8`, `i16`, `i32`, `i64`
3. `u8`, `u16`, `u32`, `u64`
4. `f16`, `bf16`, `f32`, `f64`
5. `complex64`, `complex128`

`Int`, `UInt`, `Integer`, `Float`, and `Complex` are kind-level umbrellas and are NOT matrix axes.

`Nat` and `PositiveNat` are domain scalar types and are NOT matrix axes. Conversions into those domains use the explicit prelude conversion functions and `DOMAIN_INVALID_VALUE`, not `CAST_INVALID_VALUE` or `CAST_INVALID_DTYPE_PAIR`.

`Tri` is a scalar logic type, not a v0.1 matrix axis or tensor element dtype.
`Nat` and `PositiveNat` remain outside the matrix for the domain reasons above.


### Family Matrix

Legend:

1. `F` = `FORBIDDEN`
2. `EO` = `EXPLICIT_ONLY`
3. `EL` = `EXPLICIT_LOSSY`
4. `IA` = `IMPLICIT_ALLOWED`
5. `mix` = determined by within-family rule below

| from \ to | Bool | Signed Int | Unsigned Int | Float | Complex |
|---|---|---|---|---|---|
| Bool | IA | IA | IA | IA | IA |
| Signed Int | EO | mix | mix | IA | IA |
| Unsigned Int | EO | mix | mix | IA | IA |
| Float | F | EL | EL | mix | IA |
| Complex | F | F | F | F | mix |


### Within-family Rules

#### Signed Int (`i8/i16/i32/i64`)

1. same dtype: `IA`
2. widening by range inclusion: `IA`
3. narrowing: `EL`

#### Unsigned Int (`u8/u16/u32/u64`)

1. same dtype: `IA`
2. widening by range inclusion: `IA`
3. narrowing: `EL`

#### Signed <-> Unsigned

1. if destination range fully contains source range: `IA`
2. otherwise: `EL`

#### Float (`f16/bf16/f32/f64`)

Use precedence order from `conventions.md`: `f16 < bf16 < f32 < f64`.

1. same dtype: `IA`
2. conversion to equal-or-higher precedence dtype: `IA`
3. conversion to lower precedence dtype: `EL`

#### Complex (`complex64/complex128`)

1. same dtype: `IA`
2. `complex64 -> complex128`: `IA`
3. `complex128 -> complex64`: `EL`


### Value-domain Validity (for legal pairs)

If pair legality is not `FORBIDDEN`, value validity still applies.

Violations MUST fail with `CAST_INVALID_VALUE`.

1. `Bool -> numeric`:
   1. `false` converts to numeric zero,
   2. `true` converts to numeric one,
   3. a complex destination uses that value as its real component and positive
      zero as its imaginary component.
2. `Signed Int/Unsigned Int -> Bool`:
   1. only `0` and `1` are valid
3. `Float -> Signed/Unsigned Int`:
   1. finite and in-range values are valid
   2. NaN, infinity, or out-of-range values are invalid
   3. valid conversions use truncation toward zero
4. integer narrowing:
   1. source values outside destination range are invalid
5. real-to-complex conversion:
   1. the real component follows the destination component dtype's IEEE-754
      conversion semantics,
   2. the imaginary component is positive zero in that component dtype, as
      required by `conventions.md`.


### Additional Cast Modes

#### `wrapping_cast`

`wrapping_cast` does not use the general value-domain failures above.

Rules:

1. allowed pair: fixed-width integer -> fixed-width integer
2. result uses modulo `2^bits(dst)` two's-complement semantics
3. `CAST_INVALID_VALUE` MUST NOT be emitted
4. illegal dtype pair MUST fail with `CAST_INVALID_DTYPE_PAIR`

#### `bitcast`

Rules:

1. allowed source/destination dtypes are fixed-width integer, floating, and complex dtypes
2. source and destination bit widths MUST be equal
3. bit pattern MUST be preserved exactly (reinterpret, no numeric conversion)
4. illegal dtype pair or unequal bit width MUST fail with `CAST_INVALID_DTYPE_PAIR`


### Failure Channel and Const Precedence (REQUIRED)

1. Cast operations in v0.1 are fallible and channel-stable:
   1. const-evaluable failures are compile-time errors with cast cause code identity preserved,
   2. non-const-evaluable failures MUST be runtime `Err(...)` with the same cause code identity preserved,
   3. cast failures MUST NOT use unrecoverable runtime trap/panic channel.
2. In `const` initializers, primary error precedence is fixed:
   1. non-const-evaluable initializer MUST emit `CONST_INIT_NOT_CONST` as primary error,
   2. cast legality/value errors become primary only after initializer is const-evaluable.
3. `wrapping_cast` and `bitcast` participate in const-evaluable analysis:
   1. const-evaluable legal forms MAY be folded as constants,
   2. const-evaluable illegal forms MUST emit `CAST_INVALID_DTYPE_PAIR`,
   3. `wrapping_cast` MUST NOT emit `CAST_INVALID_VALUE` in any channel.
