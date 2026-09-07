# `std::parse` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

All parsing helpers are pure and return `Res`.

```eglur-catalog
opaque type ParseError

fn parse_i8(s: Str) -> Res[i8, ParseError]
fn parse_i16(s: Str) -> Res[i16, ParseError]
fn parse_i32(s: Str) -> Res[i32, ParseError]
fn parse_i64(s: Str) -> Res[i64, ParseError]

fn parse_u8(s: Str) -> Res[u8, ParseError]
fn parse_u16(s: Str) -> Res[u16, ParseError]
fn parse_u32(s: Str) -> Res[u32, ParseError]
fn parse_u64(s: Str) -> Res[u64, ParseError]

fn parse_int(s: Str) -> Res[i64, ParseError]

fn parse_f16(s: Str) -> Res[f16, ParseError]
fn parse_bf16(s: Str) -> Res[bf16, ParseError]
fn parse_f32(s: Str) -> Res[f32, ParseError]
fn parse_f64(s: Str) -> Res[f64, ParseError]

fn parse_complex64(s: Str) -> Res[complex64, ParseError]
fn parse_complex128(s: Str) -> Res[complex128, ParseError]

fn parse_bool(s: Str) -> Res[Bool, ParseError]
```

Normative parse rules:

1. All `parse_*` helpers are pure and deterministic.
2. Leading/trailing whitespace is not ignored in v0.1:
   1. any input with leading/trailing whitespace MUST fail with `Err(ParseError)`.
3. Every finite decimal digit component uses `DecimalDigits` from the
   [source lexical grammar](../language/syntax.md#literal-tokens): components
   begin and end with an ASCII decimal digit and permit a single `_` only
   between adjacent digits. Whole-string parsing has no identifier boundary,
   so text such as `_1` is malformed numeric input.
4. Signed integer parsers (`parse_i8/i16/i32/i64`) accept `DecimalDigits` with
   an optional leading sign (`+` or `-`).
5. Unsigned integer parsers (`parse_u8/u16/u32/u64`) accept `DecimalDigits` and
   MUST reject any explicit sign.
6. Integer parsers MUST reject:
   1. malformed numeric grammar,
   2. out-of-range values for destination dtype.
7. `parse_int` is a required alias of `parse_i64` and MUST return identical
   results for the same input.
8. Floating parsers (`parse_f16/bf16/f32/f64`) accept decimal/exponent forms
   plus special tokens:
   1. case-insensitive `nan`,
   2. case-insensitive `inf`,
   3. optional leading sign for finite decimal/exponent forms and special tokens
      (`+1.0`, `-1.0e3`, `+inf`, `-inf`, `+nan`, `-nan`).
   4. every digit component in a finite form, including its exponent, obeys
      Rule 3.
9. Floating parsers MUST return `Err(ParseError)` for malformed floating grammar.
10. Complex parsers (`parse_complex64`, `parse_complex128`) support flexible text forms:
   1. real-only: `<real>`,
   2. imaginary-only: `<imag>i`,
   3. cartesian token form: `<real><separator><unsigned-imag>i`, where `<separator>` is exactly one `+` or `-`,
   4. tuple form: `(<real>,<imag>)`.
11. Complex component tokens use the destination real component parser, including its leading-sign rules:
   1. `complex64` uses `f32`-compatible components,
   2. `complex128` uses `f64`-compatible components.
12. The cartesian separator is the imaginary component sign:
   1. `<unsigned-imag>` MUST NOT contain another leading sign,
   2. `1+2i`, `1-2i`, `1+infi`, and `1-nani` are valid cartesian forms,
   3. double-sign forms such as `1+-2i`, `1-+2i`, `1++2i`, and `1--2i` MUST return `Err(ParseError)`.
13. Real-only, imaginary-only, and tuple-form components retain the leading-sign rules from Rule 11.
14. Complex forms inherit floating special-token support for components (`nan`, `inf`, signed variants where Rule 12 does not reserve the sign as the cartesian separator).
15. `parse_bool` accepts only lowercase `true` and `false` in v0.1.
