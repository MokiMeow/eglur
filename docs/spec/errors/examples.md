# Examples

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Effect Violation

```text
error[EFFECT_VIOLATION] non-bang function `f` cannot call `print!`
 --> main.eglur:3:9
 = help: mark function as `f!` or remove the effectful call
```


### Non-bang Mutable Parameter Forbidden

```text
error[NON_BANG_MUT_PARAM_FORBIDDEN] non-bang function `touch` cannot declare `&mut` parameter `c`
 --> main.eglur:2:14
 = help: make function bang or remove mutable reference parameter
```


### Reference Escape Forbidden

```text
error[REF_ESCAPE_FORBIDDEN] reference escape is forbidden in v0.1 for `r`
 --> main.eglur:4:10
 = help: keep references local and return owned values instead
```


### Return Outside Function

```text
error[RETURN_OUTSIDE_FUNCTION] return statement appears outside function body
 --> main.eglur:1:1
 = help: use return only inside fn/impl method bodies
```


### Return Value Required

```text
error[RETURN_VALUE_REQUIRED] bare return requires Unit return type
 --> main.eglur:3:5
 = help: provide return value matching function return type
```


### Return Value Forbidden

```text
error[RETURN_VALUE_FORBIDDEN] return value is forbidden in Unit-returning function
 --> main.eglur:3:5
 = help: use bare return or change function return type
```


### Return Type Mismatch

```text
error[RETURN_TYPE_MISMATCH] return expression type does not match function return type
 --> main.eglur:3:12
 = expected: i64
 = actual: Bool
 = help: return value must be assignable to declared return type
```


### Trait Impl Signature Mismatch

```text
error[TRAIT_IMPL_SIGNATURE_MISMATCH] trait impl method signature mismatch for `render`
 --> impl.eglur:8:3
 = expected: fn render(self: &Self, value: i64) -> Str
 = actual: fn render(self: &Self, value: f64) -> Str
 = help: match the trait method's generic bounds, parameters, receiver, and return type
 = related: trait method declared here
   --> traits.eglur:2:3-2:42
```


### Traverse Fail-fast Index

```text
error[TRAVERSE_RES_FAIL] traverse_res! failed at index 1: bad
 = note: index 1 is 0-based
```


### Reduce-by Phase Uses Non-reduced Axis

```text
error[REDUCE_BY_PHASE_NON_REDUCED_AXIS] reduce_by phase references non-reduced axis `b`
 --> model.eglur:9:22
 = help: use only reduced axes in phase tuples
 = note: data.axis = "b", data.phase_index = 0
```


### Reduce-by Phase Axis Overlap

```text
error[REDUCE_BY_PHASE_OVERLAP] reduce_by phase overlap on axis `h`
 --> model.eglur:10:22
 = help: assign each reduced axis to exactly one phase
 = note: data.axis = "h", data.phase_index = 1
```


### Reduce-by Phase Incomplete Coverage

```text
error[REDUCE_BY_PHASE_INCOMPLETE] reduce_by phase coverage is incomplete; missing reduced axes
 --> model.eglur:11:22
 = help: cover all reduced axes exactly once
 = note: data.expected_axes = ["h", "w"], data.actual_axes = ["h"]
```


### Axis Name Not Found

```text
error[AXIS_NAME_NOT_FOUND] axis name `'batch` not found in overlay
 --> model.eglur:12:15
```


### Axis Names Dropped

```text
warning[AXIS_NAMES_DROPPED] reshape dropped axis name(s): ['mol', 'task']
 = note: use explicit remapping when preserving names is required
```


### Immutable Binding Assignment

```text
error[IMMUTABLE_BINDING_ASSIGN] cannot assign to immutable let binding `x`
 --> main.eglur:4:3
 = help: use `let mut x = ...` if reassignment is intended
```


### Immutable Field Assignment

```text
error[IMMUTABLE_BINDING_ASSIGN] cannot assign to immutable field `n`
 --> model.eglur:6:7
 = help: declare field as `field mut n: ...` if mutation is required
```


### `var` Declaration Forbidden

```text
error[VAR_DECL_FORBIDDEN] `var` declarations are forbidden in v0.1
 --> main.eglur:2:3
 = help: replace with `let mut`
```


### Struct Field Default Forbidden

```text
error[STRUCT_FIELD_DEFAULT_FORBIDDEN] struct field default initializer is forbidden in v0.1
 --> model.eglur:3:16
 = help: move initialization to struct literal construction
```


### Const Initializer Not Const-evaluable

```text
error[CONST_INIT_NOT_CONST] const initializer is not const-evaluable
 --> core.eglur:1:15
 = help: use literal-only/const-evaluable expression in const initializer
```


### Const Forward Reference

```text
error[CONST_FORWARD_REFERENCE] const forward reference to `A` is not allowed
 --> core.eglur:1:15
 = help: declare referenced const earlier in source order
```


### Const Dependency Cycle

```text
error[CONST_CYCLE] const dependency cycle detected: A -> B -> A
 --> app/a.eglur:3:7
 = help: break const dependency cycle
```


### Ownership Path Unbalanced

```text
error[OWNERSHIP_PATH_UNBALANCED] ownership path for `x` is not discharged exactly once before its obligation region exits
 --> app.eglur:9:1
 = help: consume or transfer `x` on every edge that exits this region
```


### Ownership Profile Disabled

```text
error[OWNERSHIP_PROFILE_DISABLED] ownership profile is disabled; `own` syntax is unavailable in this build profile
 --> app.eglur:3:11
 = help: enable ownership profile or remove ownership syntax
```


### Move Requires Own

```text
error[MOVE_REQUIRES_OWN] move operand must be `Own[T]` or an allowed `Opt`/`Res` ownership carrier, found `i64`
 --> app.eglur:6:14
 = help: apply `move` only to ownership roots
```


### Own Requires Non-Own

```text
error[OWN_REQUIRES_NON_OWN] own operand must be ownership-free, found `Own[i64]`
 --> app.eglur:7:12
 = help: remove redundant own wrapping
```


### Drop-own Requires Move

```text
error[DROP_OWN_REQUIRES_MOVE] drop_own requires explicit `move` operand form
 --> app.eglur:8:7
 = help: use `drop_own move x`
```


### Resource Roots Require Effect-visible Close

```text
error[RESOURCE_DROP_FORBIDDEN] resource root `reader` cannot be discarded with drop_own
 --> app.eglur:8:7
 = help: use the resource close! operation or a scoped resource API
 = related: resource root created here
   --> app.eglur:5:3
```


### Ownership Double Move

```text
error[OWNERSHIP_DOUBLE_MOVE] ownership move is already consumed for `x` on this path
 --> app.eglur:11:15
 = help: consume each ownership root at most once per typed path
 = related: ownership root created here
   --> app.eglur:5:3-5:28
 = related: first consume occurs here
   --> app.eglur:7:12
```


### Ownership Use-after-discharge

```text
error[OWNERSHIP_USE_AFTER_DISCHARGE] ownership use references discharged root `x`
 --> app.eglur:12:16
 = help: use/borrow ownership roots only when definitely owned on all incoming paths
 = note: discharge of `x` occurs at app.eglur:8:7
```


### Struct Init Unknown Field

```text
error[STRUCT_INIT_UNKNOWN_FIELD] struct initializer has unknown field `z` for `Point`
 --> model.eglur:8:18
 = help: remove `z` or declare it on `Point`
```


### Struct Init Duplicate Field

```text
error[STRUCT_INIT_DUPLICATE_FIELD] struct initializer sets field `x` more than once
 --> model.eglur:8:23
 = help: keep exactly one initializer for each field
```


### Struct Init Missing Field

```text
error[STRUCT_INIT_MISSING_FIELD] struct initializer is missing field `y` for `Point`
 --> model.eglur:8:11
 = help: initialize all declared fields exactly once
```


### Struct Init Property Forbidden

```text
error[STRUCT_INIT_PROPERTY_FORBIDDEN] struct initializer cannot set property `count` for `Counter`
 --> model.eglur:8:20
 = help: initialize stored fields only, then call `set_count!` after construction if needed
```


### Property Accessor Missing

```text
error[PROP_ACCESSOR_MISSING] property `count` is missing required accessor `set_count!`
 --> model.eglur:5:3
 = help: declare `fn set_count!(self: &mut Self, value: i64) -> Unit` inside property body
```


### Property Accessor Signature Mismatch

```text
error[PROP_ACCESSOR_SIGNATURE_MISMATCH] property accessor signature mismatch for `set_count!`
 --> model.eglur:6:3
 = expected: fn set_count!(self: &mut Self, value: i64) -> Unit
 = actual: fn set_count!(self: &Self, value: i64) -> Unit
 = help: match property contract exactly: `fn set_count!(self: &mut Self, value: i64) -> Unit`
```


### Property Assignment Forbidden

```text
error[PROPERTY_ASSIGN_FORBIDDEN] property assignment is forbidden for `count`
 --> model.eglur:11:3
 = help: call `set_count!(...)` explicitly
```


### Overload Marker Required

```text
error[OVERLOAD_MARKER_REQUIRED] overload marker required for same-name declarations `parse`
 --> parser.eglur:6:1
 = help: mark all declarations in this overload set with `overload`
```


### Overload Visibility Mismatch

```text
error[OVERLOAD_VISIBILITY_MISMATCH] overload visibility mismatch for `parse`
 --> parser.eglur:9:1
 = note: this overload family was private at parser.eglur:6:1
 = help: give every declaration in the overload family the same visibility
```


### Overload Overlap

```text
error[OVERLOAD_OVERLAP] overload declarations for `f` overlap on argument types
 --> math.eglur:10:1
 = note: candidates: f[T](T), f(i64)
```


### Cascade Suppression

```eglur
fn take_i64(_value: i64) -> Unit {}

fn cascade() -> Unit {
  let x = missing
  _ = x.no_such_method()
  _ = take_i64(x)
}
```

The unresolved `missing` binding is a `NAME_NOT_FOUND` root. The inferred type
of `x` is poisoned, so method lookup cannot establish `METHOD_NOT_FOUND` from a
known receiver type and argument checking cannot establish
`CALL_ARG_TYPE_MISMATCH`. Both candidates are derivative and are omitted.


### Independent Roots After Recovery

```eglur
fn first() -> i64 = missing
fn second() -> i64 = "text"
```

`NAME_NOT_FOUND` in `first` and `RETURN_TYPE_MISMATCH` in `second` are
independent roots. An implementation that recovers across the declaration
boundary retains both in source order. The v0.1 baseline permits an
implementation to stop after the first root, so this example illustrates the
recommended recovery target rather than a required diagnostic count.
