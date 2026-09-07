# Unsafe and FFI Boundary

Scope: Normative for v0.1 unless this file or a section states otherwise.

Low-level and foreign calls MUST require an explicit `unsafe` boundary.

Safe code paths SHOULD remain possible without user-authored `unsafe`.

## v0.1 Surface (REQUIRED)

v0.1 defines the following unsafe surface syntax (see `syntax.md`):

1. `unsafe` block expression: `unsafe { ... }`.
2. Foreign declaration item: `extern "<abi>" fn name(...) -> T`.

`unsafe fn` is not part of v0.1 baseline surface.

## v0.1 Normative Rules

1. Calling a function declared by `extern "<abi>" fn ...` is an unsafe operation.
2. An unsafe operation MUST appear lexically inside an `unsafe { ... }` block.
3. Calling an extern function outside `unsafe { ... }` MUST fail at compile time with `UNSAFE_REQUIRED`.
4. Extern declarations are module-internal boundary declarations in v0.1:
   1. `pub extern ...` is forbidden and MUST fail with `EXTERN_VISIBILITY_FORBIDDEN`,
   2. cross-module access to extern symbols is not part of v0.1 surface; modules MUST expose wrapper APIs (`pub fn`/`pub fn!`) instead.
5. Extern call effect classification is declaration-kind-based:
   1. extern symbols are always effectful call targets in v0.1, regardless of callee identifier suffix,
   2. extern symbol names are ABI-facing and therefore exempt from the normal `!` suffix naming convention in `effects.md`,
   3. calling extern from a non-bang function/method MUST fail with `EFFECT_VIOLATION`.
6. `unsafe` boundary does not relax effect rules:
   1. `unsafe` and bang (`!`) are independent axes,
   2. if a call is effectful it still follows bang rules from `effects.md`.
7. `unsafe` boundary is lexical and non-transitive across declarations:
   1. being inside `unsafe { ... }` does not implicitly mark called functions as unsafe-allowed,
   2. each unsafe call site must be within an active unsafe block.
8. `unsafe { ... }` uses ordinary `BlockExpr` result semantics from `control-flow.md`; a value-producing unsafe block MUST use `yield`.
9. Extern ABI selector subset is constrained in v0.1:
   1. only `"C"` is part of baseline ABI selector surface,
   2. any other ABI string MUST fail with `EXTERN_ABI_UNSUPPORTED`.
10. Extern signature subset is constrained in v0.1:
   1. direct parameter and return positions allow the numeric ABI scalar leaf types `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, and `f64`,
   2. `Bool` is allowed only in direct parameter and return positions:
      1. under ABI `"C"`, direct `Bool` values map to C `_Bool`,
      2. outbound `false` and `true` map to C `_Bool` values `0` and `1`, respectively,
      3. inbound C `_Bool` false and true values map to Eglur `false` and `true`, respectively,
      4. this direct-value mapping does not define an addressable C-compatible storage layout for Eglur `Bool`,
   3. `Unit` is allowed only as a direct return type and maps to C `void`,
   4. allowed pointer forms are `*const T` and `*mut T`, in parameter or return position, where `T` is one of the numeric ABI scalar leaf types from Rule 10.1,
   5. `Unit` parameters and pointers whose pointee is `Bool` or `Unit` MUST fail with `EXTERN_SIGNATURE_UNSUPPORTED`,
   6. `*const Unit` and `*mut Unit` are not aliases for C `const void*` or `void*`; v0.1 defines no C `void` pointee type,
   7. nominal user-defined types are not part of v0.1 extern signature subset:
      1. `struct`/`enum` types MUST NOT appear as extern parameter or return types,
      2. pointers to `struct`/`enum` types are also outside v0.1 baseline subset,
      3. built-in residual/ownership/layout families (`Opt[...]`, `Res[...]`, `Own[...]`, `Box[...]`) are also outside v0.1 subset,
      4. `Never` is outside the direct parameter, direct return, and raw-pointer
         pointee subset; v0.1 defines no foreign noreturn contract,
   8. references (`&T`, `&mut T`), function types, trait-object types, and tuple payload ABI are not part of v0.1 extern signature subset,
   9. varargs are not part of the v0.1 `ExternDecl` grammar surface,
   10. extern declarations MUST include an explicit return type; omitted return type MUST fail with `FN_RETURN_TYPE_REQUIRED`,
   11. omitted extern return type is not implicitly `Unit`; declarations with no ABI result MUST write `-> Unit`,
   12. unsupported extern parameter or return types that parse as `TypeExpr` MUST fail with `EXTERN_SIGNATURE_UNSUPPORTED`,
   13. `Never` bottom compatibility does not make an otherwise unsupported
       extern signature or raw-pointer relay argument legal.
11. Source-written raw pointer types outside extern declaration signature positions are not part of v0.1 baseline:
   1. ordinary function/method signatures, local annotations, field/property declarations, type aliases, trait declarations, and generic arguments MUST NOT use `*const T` or `*mut T`,
   2. such non-extern raw pointer type expressions MUST fail with `RAW_POINTER_TYPE_UNSUPPORTED`,
   3. the inferred relay exception below does not make `PtrType` valid in any additional source type position.
12. A raw-pointer **relay region** is the body of the outermost active `unsafe` expression within one callable:
   1. entering a nested `unsafe` while a relay region is already active does not start a smaller region,
   2. the region ends when control or a value leaves that outermost `unsafe` body,
   3. a function, method, closure, or child-task body never inherits an enclosing relay region.
13. A compiler-inferred raw-pointer relay value may originate only from a call to a valid extern declaration whose return type is an allowed `*const T` or `*mut T`. No literal, null value, cast, address-of form, dereference, or other pointer constructor is part of v0.1.
14. Within one active relay region, a raw-pointer value MAY appear only in these flows:
   1. directly as a compatible extern-call argument, including one extern pointer result passed directly to another extern call,
   2. through parentheses or through a non-aggregate `BlockExpr`, `if`, or `match` result whose pointer-producing paths remain in the same relay region and have the same exact raw-pointer type,
   3. as the initializer or source of an unannotated immutable identifier binding `let name = <expr>`; repeated reads and further bindings of this form are allowed in the same region,
   4. in a no-binding discard: `_ = <expr>`, `let _ = <expr>`, or an expression statement whose value is discarded,
   5. from `*mut T` to an extern `*const T` parameter for the same `T`.
15. No other raw-pointer value flow is part of v0.1. The first such use MUST fail with `RAW_POINTER_USE_UNSUPPORTED`. Rejected uses include:
   1. the result of the outermost relay-region `unsafe` expression or any callable, closure, or task result,
   2. a `let mut` binding, assignment target/value, or other mutable storage,
   3. tuple, array, struct, enum, residual, collection, or other aggregate construction or storage,
   4. explicit or inferred generic instantiation, an ordinary function/method/callback argument, or callable construction,
   5. closure or child-task capture,
   6. comparison, hashing, pattern matching, or use as a control-flow scrutinee,
   7. dereference, arithmetic, address-of, cast/conversion, member access, indexing, or callee use.
16. Relay pointer compatibility is exact except for Rule 14.5:
   1. `*const T` never converts to `*mut T`,
   2. pointers with different pointee types never convert,
   3. an otherwise permitted extern argument with an incompatible raw-pointer type MUST fail with `CALL_ARG_TYPE_MISMATCH`,
   4. pointer-producing `if` or `match` branches form a relay result only when every pointer-producing branch has the same exact raw-pointer type; otherwise the ordinary `IF_BRANCH_TYPE_MISMATCH` or `MATCH_ARM_TYPE_MISMATCH` rule applies,
   5. Rule 14.5 is an extern-argument conversion only and MUST NOT provide a common type for a branch join.
17. A relay value is an unmanaged copyable foreign address identity, not an Eglur reference or ownership root:
   1. copying or repeatedly reading it copies the same address identity and is a narrow exception to the ordinary no-observable-alias value guarantee,
   2. copying, discarding, or leaving its lexical scope does not retain, release, validate, invalidate, or otherwise affect foreign storage,
   3. `unsafe` and relay confinement establish no null, lifetime, provenance, allocation, deallocation, exclusivity, alignment, or pointee-validity guarantee.
18. Raw-pointer diagnostic precedence is fixed:
   1. parse and name-resolution failures retain their ordinary earlier phases,
   2. `UNSAFE_REQUIRED`, `EFFECT_VIOLATION`, and extern declaration ABI/signature failures precede relay-flow validation,
   3. a source-written non-extern pointer type expression uses `RAW_POINTER_TYPE_UNSUPPORTED`,
   4. branch-result typing under Rule 16.4 precedes relay-flow validation because no relay value exists until the branches have one exact common type,
   5. a forbidden use of an otherwise valid inferred relay value uses `RAW_POINTER_USE_UNSUPPORTED` before ordinary return, aggregate, generic, capture, comparison, or operator diagnostics,
   6. Rule 16.3 applies only after the argument is established as an otherwise permitted extern sink.
19. Linkage failure channel split and code identity are normative:
   1. linkage/symbol-resolution failures MUST use cause code `EXTERN_LINKAGE_FAILED`,
   2. statically decidable linkage failure MUST surface in `compile_error`,
   3. non-statically-decidable linkage failure MUST surface in `runtime_error`.

## Examples

Allowed:

```eglur
extern "C" fn c_abs(x: i64) -> i64
extern "C" fn c_is_ready(flag: Bool) -> Bool
extern "C" fn c_finish() -> Unit

fn abs_via_ffi!(x: i64) -> i64 {
  yield unsafe { yield c_abs(x) }
}
```

A pointer-returning extern may be relayed locally, reused, weakened from
`*mut f64` to an extern `*const f64` parameter, and released through another
extern call while the outer `unsafe` region remains active:

```eglur
extern "C" fn c_alloc_values() -> *mut f64
extern "C" fn c_sum_values(values: *const f64) -> f64
extern "C" fn c_free_values(values: *mut f64) -> Unit

fn sum_and_release!() -> f64 {
  yield unsafe {
    let values = c_alloc_values()
    let alias = values
    let total = c_sum_values(alias)
    _ = alias
    c_free_values(values)
    yield total
  }
}
```

Rejected:

```eglur
extern "C" fn c_abs(x: i64) -> i64

fn abs_via_ffi!(x: i64) -> i64 {
  yield c_abs(x)  // compile_error: UNSAFE_REQUIRED
}
```

```eglur
pub extern "C" fn c_abs(x: i64) -> i64
// compile_error: EXTERN_VISIBILITY_FORBIDDEN
```

```eglur
extern "C" fn c_take_ref(x: &i64) -> i64
// compile_error: EXTERN_SIGNATURE_UNSUPPORTED
```

```eglur
extern "C" fn c_take_bool_ptr(x: *const Bool) -> Unit
// compile_error: EXTERN_SIGNATURE_UNSUPPORTED
```

```eglur
extern "C" fn c_take_unit(x: Unit) -> Unit
// compile_error: EXTERN_SIGNATURE_UNSUPPORTED
```

```eglur
enum Tag { A B }
extern "C" fn c_take_tag(x: Tag) -> i64
// compile_error: EXTERN_SIGNATURE_UNSUPPORTED
```

```eglur
extern "system" fn c_abs(x: i64) -> i64
// compile_error: EXTERN_ABI_UNSUPPORTED
```

```eglur
fn take_ptr(p: *const u8) -> i64 { yield 0 }
// compile_error: RAW_POINTER_TYPE_UNSUPPORTED
```

```eglur
extern "C" fn c_alloc_values() -> *mut f64

fn escaped!() -> i64 {
  let values = unsafe { yield c_alloc_values() }
  // compile_error: RAW_POINTER_USE_UNSUPPORTED
  _ = values
  yield 0
}
```

```eglur
extern "C" fn c_alloc_values() -> *mut f64

fn stored!() -> i64 {
  yield unsafe {
    let values = c_alloc_values()
    let pair = (values, values)
    // compile_error: RAW_POINTER_USE_UNSUPPORTED
    _ = pair
    yield 0
  }
}
```

FFI ABI policy is fixed for v0.1 baseline:

1. ABI selector subset: `"C"` only.
2. Linkage failure cause code: `EXTERN_LINKAGE_FAILED` (channel selected by static decidability rules).

In v0.1 baseline, foreign-call boundary is the only standardized unsafe operation category.
Additional low-level unsafe primitives MAY be added only in extension/non-baseline profiles.

v0.1 defines no capability-token authorization requirement for `unsafe`.
