# `std::json` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

`std::json` provides pure JSON value construction, parsing, projection, and serialization.

```eglur-catalog
opaque type Json
opaque type JsonError

fn parse_json(s: Str) -> Res[Json, JsonError]
fn stringify_json(v: &Json) -> Str

fn json_null() -> Json
fn json_bool(v: Bool) -> Json
fn json_number(v: f64) -> Res[Json, JsonError]
fn json_str(v: Str) -> Json
fn json_array(xs: Seq[Json]) -> Json
fn json_object(entries: Seq[(Str, Json)]) -> Res[Json, JsonError]

fn Json.is_null(self: &Self) -> Bool
fn Json.as_bool(self: &Self) -> Opt[Bool]
fn Json.as_number(self: &Self) -> Opt[f64]
fn Json.as_str(self: &Self) -> Opt[Str]
fn Json.as_array(self: &Self) -> Opt[Seq[Json]]
fn Json.as_object(self: &Self) -> Opt[Map[Str, Json]]
```

`Json` and `JsonError` do not implement `Eq` in v0.1. JSON structural
equality, including numeric and object-member identity, is deferred rather than
being inferred from the opaque representation.

Normative rules:

1. All `std::json` APIs in this module are pure and deterministic.
2. `parse_json` recognizes RFC-8259 JSON grammar, subject to the value restrictions below, for:
   1. `null`, `true`, `false`,
   2. numbers,
   3. strings with JSON escape rules,
   4. arrays,
   5. objects.
3. v0.1 baseline parser does not accept non-JSON extensions:
   1. comments,
   2. trailing commas,
   3. single-quoted strings.
4. `parse_json` MUST return `Err(JsonError)` when one object literal contains duplicate member names.
   1. duplicate-key rejection applies recursively to nested object literals as well.
5. JSON lexical numbers convert to the `f64` number representation as follows:
   1. the mathematical decimal value denoted by the complete JSON number token MUST be rounded once to the nearest representable IEEE-754 binary64 value using `roundTiesToEven`,
   2. finite precision loss is accepted; integer, fraction, and exponent token forms do not create distinct runtime number kinds,
   3. every finite rounded result, including a subnormal value or zero produced by underflow, is accepted,
   4. an exact or underflowed zero MUST preserve the token sign: a token with a leading `-` produces negative zero and every other zero token produces positive zero,
   5. if the correctly rounded result is positive or negative infinity, `parse_json` MUST return `Err(JsonError)`,
   6. implementations MAY use any conversion algorithm, but the resulting `f64` bit pattern or `Err` outcome MUST be identical to this rule.
6. Parse failures MUST return `Err(JsonError)` in value channel and MUST NOT be reported as runtime abort.
7. `stringify_json` MUST return compact canonical JSON text:
   1. no insignificant whitespace,
   2. lowercase JSON keywords (`null`, `true`, `false`),
   3. string escaping and number rendering follow the canonical rules below.
8. Object member order is stable and deterministic:
   1. `parse_json` preserves source member order in parsed object values,
   2. `json_object` preserves provided entry order,
   3. `stringify_json` emits members in stored order.
9. Canonical number rendering for `stringify_json`:
   1. finite numbers MUST use the same finite decimal selection as canonical `to_str(f64)` in `std::prelude`,
   2. JSON number rendering MUST NOT emit the floating-point `.0` cue used by `to_str(f64)`,
   3. positive finite zero formats as `0`,
   4. negative finite zero formats as `-0`,
   5. exponent notation MUST use lowercase `e`, omit `+` for positive exponents, and omit leading zeros in the exponent digits,
   6. insignificant trailing fractional zeros MUST NOT be emitted,
   7. a trailing decimal point MUST NOT be emitted,
   8. examples:
      1. `1.0 -> "1"`,
      2. `4.50 -> "4.5"`,
      3. `2e-3 -> "0.002"`,
      4. `-0.0 -> "-0"`.
10. Canonical string escaping for `stringify_json`:
   1. `"` and `\` MUST be escaped as `\"` and `\\`,
   2. control characters with dedicated short JSON escapes MUST use those short escapes:
      1. backspace `\b`,
      2. form feed `\f`,
      3. newline `\n`,
      4. carriage return `\r`,
      5. tab `\t`,
   3. other control characters in `U+0000..U+001F` MUST use lowercase `\u00xx`,
   4. `/` MUST NOT be escaped,
   5. non-ASCII scalar values MUST be emitted directly as UTF-8 and MUST NOT be force-escaped as `\uXXXX`.
11. `json_object` MUST return `Err(JsonError)` when duplicate keys appear in one object construction call.
12. `json_number` accepts only finite `f64` values:
   1. `NaN` and `±Inf` MUST return `Err(JsonError)`.
13. Projection accessors (`as_*`) are exact-kind:
   1. they return `Some(...)` only for matching JSON kind,
   2. otherwise they return `None`.
14. `Json.is_null` returns `true` iff the value is JSON null.
15. v0.1 `std::json` is DOM-only:
   1. typed encode/decode helpers (for example `to_json[T]`, `from_json[T]`, `parse_json_as[T]`) are not part of v0.1 baseline,
   2. baseline conformance MUST NOT require derive/protocol/reflection-based JSON mapping,
   3. any implementation-defined typed JSON mapping extension MUST treat `Json` as the canonical interchange boundary rather than bypassing it with a separate runtime representation.
