# Required Runtime and Stdlib Diagnostics

Scope: Normative for v0.1 unless this file or a section states otherwise.

This section defines diagnostics emitted by fallible/effectful stdlib behavior and traversal semantics.

`diagnostic-contracts.yaml` is normative for severity, message, and `data`
fields. The per-code field lists below are explanatory summaries; trigger and
channel rules remain normative prose.

Codes in this section identify failure causes, not an exclusive channel.

When a failure cause here is statically decidable (see `../language/validation-matrix.md`), implementations MUST use the same code in `compile_error` channel.

Channel selection and cross-channel requirements are normative in `code-channels.md`.

Data contract summaries in this file use the diagnostic model's serialized
`i64` numeric payload scalar.
This is a diagnostic interchange convention, not a user-callable API boundary, and does not override `Nat`/`PositiveNat` parameter requirements in the language or stdlib specs.

### `TRAVERSE_RES_FAIL`

Severity: `error`

Trigger: `traverse_res`, `sequence_res`, `traverse_res!`, `par_traverse_res!` short-circuits on `Err`.

Message contract summary:

1. `index <N>` where `<N>` is 0-based.

Data contract summary:

1. `index` (i64, 0-based).


### `TRAVERSE_OPT_NONE`

Severity: `note`

Trigger: `traverse_opt`, `sequence_opt`, `traverse_opt!`, `par_traverse_opt!` short-circuits on `None`.

Baseline v0.1 emission policy: optional emit-if-present.

Message contract summary:

1. `index <N>` where `<N>` is 0-based.

Data contract summary:

1. `index` (i64, 0-based).

### `FOLD_RES_FAIL`

Severity: `error`

Trigger: `fold_res` or `fold_res!` short-circuits on `Err`.

Message contract summary:

1. `index <N>` where `<N>` is 0-based.

Data contract summary:

1. `index` (i64, 0-based).


### `AXIS_OUT_OF_RANGE`

Severity: `error`

Trigger: axis selector contains positional index `#i` where `i >= rank`.

Message contract summary:

1. `axis`.
2. `rank`.

Data contract summary:

1. `axis`.
2. `rank`.


### `AXIS_NAME_NOT_FOUND`

Severity: `error`

Trigger: name-based axis operation requests an axis name missing from `NameOverlay`.

Message contract summary:

1. Missing axis name.
2. Phrase indicating name lookup failed (for example `not found`).

Data contract summary:

1. `name`.


### `AXIS_DUPLICATE_NAME`

Severity: `error`

Trigger: `with_names`/`rename` (or equivalent) would produce non-unique `AxisName` values.

For `with_names`, uniqueness is checked against the final overlay after simultaneously applying the complete patch and preserving omitted existing entries. If several names are duplicated, the primary diagnostic reports the name whose second occurrence is encountered first in increasing `AxisIndex` order.

Message contract summary:

1. `duplicate`.
2. Duplicated name when printable.


### `AXIS_ALIASED_HEADER`

Severity: `error`

Profile: non-baseline strict-alias extension only.

Trigger: in strict-alias extension profiles, optional-internal shared-header states are surfaced and an in-place metadata mutation (`with_names!`/`rename!`) is attempted without permitted header split.

Message contract summary:

1. `aliased`.
2. `header`.


### `AXIS_MISMATCH`

Severity: `error`

Trigger: strict-name operations (for example strict contract mode) encounter incompatible axis-name bindings, including invalid `OpSchemaN.with_aliases` configuration or incompatibility after alias canonicalization.

Message contract summary:

1. Operation name.
2. Expected and actual axis-name sets or binding summary.

Data contract summary:

1. `operation`.
2. `expected_axes`.
3. `actual_axes`.


### `CONTRACT_REPEATED_LABEL_DIM_MISMATCH`

Severity: `error`

Trigger: contract evaluation encounters repeated occurrences of one axis label whose concrete dimensions are not equal.

Selection contract:

1. Distinct labels are checked in first-appearance order over left-hand-side
   operand lists.
2. Occurrences of one label are checked in operand order and then source-axis
   order.
3. The first occurrence supplies `expected_dim`; the first later unequal
   occurrence supplies `actual_dim`.
4. The earliest mismatch selected by Rules 1-3 supplies the diagnostic `label`.

Message contract summary:

1. `contract`.
2. `repeated`.
3. `label`.
4. Offending axis label when available.
5. `mismatch`.

Data contract summary:

1. `operation`.
2. `label`.
3. `expected_dim`.
4. `actual_dim`.


### `AXIS_NAMES_DROPPED`

Severity: `warning`

Trigger: operation intentionally drops one or more axis names (for example any shape-changing primitive `reshape` or an indexing transformation that removes axis identity).

Message contract summary:

1. `dropped`.
2. `name`.


### `AXIS_OVERLAY_PERMUTED`

Severity: `note`

Trigger: successful `permute` on a tensor with non-empty overlay.

Message contract summary:

1. `permute`.
2. Result overlay summary.


### `DUPLICATE_KEY`

Severity: `error`

Trigger: `Seq[(K,V)].to_map()` encounters duplicate key.

Message contract summary:

1. `duplicate`.
2. Key value when printable.

Data contract summary:

1. `key` when representable.


### `WITH_SIZES_UNKNOWN_KEY`

Severity: `error`

Trigger: a `.with_sizes(...)` map contains a key that does not name a scalar dimension variable in the receiver schema's input or output axis expressions. Named and anonymous axis packs are not scalar dimension variables for this purpose.

Unknown-key validation precedes dimension solving. Const-evaluable occurrences use this code in `compile_error`; dynamic occurrences surface from schema invocation through the operation's existing `Err(...)` family. If several keys are unknown, the first in `Map` insertion order is reported.

Message contract summary:

1. `with_sizes`.
2. `unknown`.

Data contract summary:

1. `key`.
2. `operation`.


### `AMBIGUOUS_DIMS`

Severity: `error`

Trigger: transform solver finds two or more valid solutions for axis/dimension constraints, including:

1. axis-expression dimension-variable solving, or
2. axis-pack partition solving.

Message contract summary:

1. `ambiguous`.
2. `dims`.

Data contract summary:

1. `operation`.


### `INCONSISTENT_DIMS`

Severity: `error`

Trigger:

1. transform dim solver finds no valid `Nat` solution, or
2. its unique solution requires a nonzero `AxisExpr` extent outside the
   `Nat` range during pre-traversal coordinate-map construction.

Message contract summary:

1. `inconsistent`.
2. `dims`.

Data contract summary:

1. `operation`.


### `DIM_BINDING_REQUIRED`

Severity: `error`

Trigger: after applying explicit `.with_sizes(...)` bindings and the complete
v0.1 dimension-inference procedure, one or more unresolved variables remain in
a multivariate equation component that the baseline solver does not classify
as inconsistent, uniquely solved, or mathematically ambiguous.

This code reports an intentional inference boundary, not a claim that the
underlying equation has multiple solutions. Users MUST be able to make progress
by binding enough listed variables through `.with_sizes(...)` to leave only
ground or univariate equation components.

Message contract summary:

1. `dim`.
2. `binding`.
3. `required`.

Data contract summary:

1. `operation`.
2. `variables` in first source-occurrence order.


### `BROADCAST_SHAPE_MISMATCH`

Severity: `error`

Trigger: either:

1. baseline common-shape broadcasting encounters incompatible non-singleton dimensions, or
2. directed expansion encounters a target rank smaller than the source rank or an aligned source dimension that is neither equal to the target dimension nor `1`.

For common-shape broadcasting, the primary diagnostic identifies the lowest incompatible normalized output axis. When an implementation exposes an operand pair, it selects the first incompatible pair in source order.

Message contract summary:

1. `broadcast`.
2. `shape`.
3. `mismatch`.

Data contract summary:

1. `operation`.


### `MULTI_INPUT_NOT_ALLOWED`

Severity: `error`

Trigger: an operation constrained to unary input in v0.1 (for example `view`) is called with multiple input operands.

Message contract summary:

1. `multi-input`.
2. Operation name.


### `NOT_A_VIEW`

Severity: `error`

Trigger: an affine view operation cannot be represented without copying. This
includes `view{...}` and `reshape(TensorView[T], ...)`; the owning
`reshape(Tensor[T], ...)` overload does not use this failure.

Message contract summary:

1. `view`.
2. `affine`.


### `NUMEL_MISMATCH_GROW`

Severity: `error`

Trigger: non-duplicating transform (for example `rearrange`) implies output element count greater than total input element count.

Message contract summary:

1. `numel`.
2. `grow`.


### `NUMEL_MISMATCH_SHRINK`

Severity: `error`

Trigger: non-reducing transform (for example `rearrange`) implies output element count smaller than total input element count.

Message contract summary:

1. `numel`.
2. `shrink`.


### `SEGMENT_STRADDLE`

Severity: `error`

Trigger: a multi-input structural mapping crosses operand boundaries, either
because `+` segment ownership interleaves/straddles input roots or because one
`rearrange` product node consumes factors from multiple input roots.
Operand-root provenance is checked after the closed structural proof, so this
code also applies when an admitted factoring proof produces such a mixed-root
product; that case does not use `AXIS_MAPPING_UNSUPPORTED`.

Message contract summary:

1. `segment`.
2. `straddle`.


### `REDUCTION_EMPTY_DOMAIN`

Severity: `error`

Trigger: primitive reduction-form `max` has normalized selected-axis reduction
volume `0`. The failure applies even when a zero-length retained axis makes the
prospective output tensor empty. An empty selected-axis set has volume `1` and
does not trigger this code.

Const-evaluable occurrences use this code in `compile_error`. Runtime-dependent
occurrences return `Err(ShapeError)` with the same code. If multiple selected
axes have extent `0`, the reported axis is the lowest source `AxisIndex` after
selector normalization.

Message contract summary:

1. `reduction`.
2. `empty`.
3. `domain`.

Data contract summary:

1. `operation` (`max` in v0.1).
2. `axis` (lowest selected zero-length source axis).


### `MAP_AXIS_IN_AXES_ARITY_MISMATCH`

Severity: `error`

Trigger: `in_axes` length does not match mapped function arity.

Message contract summary:

1. `in_axes`.
2. `arity`.
3. `mismatch`.

Data contract summary:

1. `expected_count`: mapped function arity.
2. `actual_count`: `in_axes` length.


### `MAP_AXIS_NO_MAPPED_ARGS`

Severity: `error`

Trigger: `map_axis` call provides no mapped arguments (all `in_axes=None`).

Message contract summary:

1. `map_axis`.
2. `mapped`.
3. `none`.


### `MAP_AXIS_NON_TENSOR_MAPPED`

Severity: `error`

Trigger: a non-tensor argument is marked mapped by non-`None` `in_axes`.

Message contract summary:

1. `non-tensor`.
2. `mapped`.


### `MAP_AXIS_BATCH_SIZE_MISMATCH`

Severity: `error`

Trigger: mapped axes do not share identical batch size.

Message contract summary:

1. `axis`.
2. `size`.
3. `mismatch`.

Data contract summary:

1. `index`.
2. `axis`.


### `MAP_AXIS_OUT_AXIS_OUT_OF_RANGE`

Severity: `error`

Trigger: `out_axis` is outside insertion range `0..r` where `r` is per-step result rank.

Message contract summary:

1. `out_axis`.
2. `range`.

Data contract summary:

1. `axis`.
2. `rank`.


### `MAP_AXIS_RESULT_SHAPE_MISMATCH`

Severity: `error`

Trigger: a later successful mapped result shape differs from baseline shape.

The primary diagnostic MUST identify the earliest mismatching 0-based result index.

Message contract summary:

1. `shape`.
2. `mismatch`.
3. `index`.

Data contract summary:

1. `index`.


### `MAP_AXIS_RESULT_OVERLAY_MISMATCH`

Severity: `error`

Trigger: after matching the baseline shape, a later successful mapped result has different `AxisIndex -> AxisName` bindings from the first successful result. `Map` insertion order alone is not a mismatch.

The primary diagnostic MUST identify the earliest mismatching 0-based result index and summarize both the expected baseline overlay and the actual overlay.

Message contract summary:

1. `overlay`.
2. `mismatch`.
3. `index`.
4. Expected and actual overlay summaries.

Data contract summary:

1. `index`.
2. `expected_overlay`.
3. `actual_overlay`.


### `MAP_AXIS_OUT_NAME_DUPLICATE`

Severity: `error`

Trigger: `out_name` duplicates an existing output axis name.

Message contract summary:

1. `out_name`.
2. `duplicate`.


### `MAP_AXIS_EMPTY_BATCH_UNSUPPORTED`

Severity: `error`

Trigger: v0.1 `map_axis` call has batch size 0 and implementation cannot infer stacked output shape.

Message contract summary:

1. `empty`.
2. `batch`.
3. `unsupported`.


### `MAP_AXIS_RES_FAIL`

Severity: `error`

Trigger: `map_axis` over `Res`-returning function short-circuits at first `Err`.

Message contract summary:

1. `index <N>` where `<N>` is 0-based.

Data contract summary:

1. `index` (i64, 0-based).


### `MAP_AXIS_OPT_NONE`

Severity: `note`

Trigger: `map_axis` over `Opt`-returning function short-circuits at first `None`.

Baseline v0.1 emission policy: optional emit-if-present.

Message contract summary:

1. `index <N>` where `<N>` is 0-based.

Data contract summary:

1. `index` (i64, 0-based).


### `SCATTER_SET_DUPLICATE_INDEX`

Severity: `error`

Trigger: `set_at(..., mode=ScatterMode::Assign)`,
`set_at!(..., mode=ScatterMode::Assign)`, or equivalent `SCATTER_SET` receives
duplicate target indices.

Message contract summary:

1. `scatter`.
2. `duplicate`.
3. `index`.

Data contract summary:

1. `index`.


### `INDEX_OUT_OF_BOUNDS`

Severity: `error`

Trigger: an `IndexItem::At` or `IndexItem::Take` coordinate remains outside
valid bounds for at least one indexed axis after signed coordinate
normalization. `IndexItem::Slice` bounds are clipped and do not trigger this
code.

Message contract summary:

1. `index`.
2. `out`.
3. `bounds`.


### `INDEX_SPEC_ARITY_MISMATCH`

Severity: `error`

Trigger: after counting the source-axis-consuming items in an `IndexSpec` or
`Vec[RangeSpec]`, the count is greater than the indexed tensor's source rank.
Implicit trailing `All` completion and `IndexItem::Rest` never trigger this code
for an under-specified vector.

Dynamic occurrences surface as `Err(IndexError)`. Const-evaluable occurrences
use the same code in `compile_error`.

Message contract summary:

1. `index`.
2. `spec`.
3. `arity`.
4. `mismatch`.

Data contract summary:

1. `source_rank`.
2. `consuming_items`.


### `INDEX_SPEC_REST_DUPLICATE`

Severity: `error`

Trigger: an `IndexSpec` or `Vec[RangeSpec]` contains more than one
`IndexItem::Rest`. This validation precedes source-rank arity validation.

Dynamic occurrences surface as `Err(IndexError)`. Const-evaluable occurrences
use the same code in `compile_error`.

Message contract summary:

1. `index`.
2. `spec`.
3. `Rest`.
4. `duplicate`.

Data contract summary:

1. `count`.


### `SLICE_STEP_ZERO`

Severity: `error`

Trigger: `IndexItem::Slice(start, stop, step)` is evaluated with `step == 0` by
`slice`, `get_at`, `set_at`, or `set_at!`.

Dynamic occurrences surface as `Err(IndexError)`. Const-evaluable occurrences use the same code in `compile_error`.

Message contract summary:

1. `slice`.
2. `step`.
3. `zero`.

Data contract summary:

1. `value`.


### `INT_OVERFLOW`

Severity: `error`

Trigger:

1. checked fixed-width integer arithmetic result is not representable in destination dtype;
2. implicit integer conversion required by numeric promotion cannot represent operand value in promoted destination dtype;
3. an integer literal's mathematical value is not representable in its
   contextually established or default fixed-width dtype;
4. a checked fixed-width integer `ScatterMode::Add` update is not representable
   in the destination tensor dtype;
5. a checked fixed-width integer reduction fold step is not representable in
   the reduction result dtype.

Dynamic scalar occurrences surface as `Err(NumericError)` from the enclosing `checked {}` block. Tensor occurrences surface through the operation's declared `Res[..., NumericError]` channel. Const-evaluable occurrences use the same code in `compile_error`.
Integer literal representability failures are always statically decidable and
therefore use `compile_error`.

Message contract summary:

1. `int`.
2. `overflow`.

Data contract summary:

1. `operation`: canonical lowercase operation identity. Tensor primitive
   `std::tensor::prim::add` uses `add`, including a failure during its promotion
   conversion; primitive `sum` uses `sum`; the default `reduce{...}` reducer uses
   `reduce`; `contract{...}` uses `contract`; `einop{...}` uses `einop`; and
   `ScatterMode::Add` uses `scatter_add`.
2. `dtype`: canonical lowercase fixed-width destination dtype token, such as
   `i8`.


### `INT_DIV_BY_ZERO`

Severity: `error`

Trigger: integer division is evaluated with divisor equal to zero.

Dynamic scalar occurrences surface as `Err(NumericError)` from the enclosing `checked {}` block. Tensor occurrences surface through the operation's declared `Res[..., NumericError]` channel. Const-evaluable occurrences use the same code in `compile_error`.

Message contract summary:

1. `int`.
2. `div`.
3. `zero`.

Data contract summary:

1. `operation`.
2. `dtype`.


### `INT_REM_BY_ZERO`

Severity: `error`

Trigger: integer remainder is evaluated with divisor equal to zero.

Dynamic scalar occurrences surface as `Err(NumericError)` from the enclosing `checked {}` block. Tensor occurrences surface through the operation's declared `Res[..., NumericError]` channel. Const-evaluable occurrences use the same code in `compile_error`.

Message contract summary:

1. `int`.
2. `rem`.
3. `zero`.

Data contract summary:

1. `operation`.
2. `dtype`.


### `CAST_INVALID_VALUE`

Severity: `error`

Trigger: cast input value is outside the accepted value-domain for an otherwise legal source/destination pair under v0.1 cast rules.

Message contract summary:

1. `cast`.
2. `invalid`.
3. `value`.

Data contract summary:

1. `src_dtype`.
2. `dst_dtype`.


### `DOMAIN_INVALID_VALUE`

Severity: `error`

Trigger: domain scalar construction or conversion receives a value outside the accepted mathematical range for the target domain type.

Message contract summary:

1. `domain`.
2. `invalid`.
3. `value`.

Data contract summary:

1. `domain`.
2. `value`.


### `CAST_INVALID_DTYPE_PAIR`

Severity: `error`

Trigger:

1. cast operation uses a source/destination dtype pair that is illegal for that cast mode;
2. `bitcast` source/destination bit widths are unequal.

Message contract summary:

1. `cast`.
2. `dtype`.
3. `pair`.

Data contract summary:

1. `src_dtype`.
2. `dst_dtype`.


### `DETERMINISM_UNAVAILABLE`

Severity: `error`

Trigger: strict determinism is explicitly requested through a profile-bearing
API for an execution path/primitive that the current backend-device context
cannot guarantee.

Message contract summary:

1. `determinism`.
2. `strict`.
3. `unavailable`.

Data contract summary:

1. `profile`.
2. `backend`.
3. `device`.

### `EXTERN_LINKAGE_FAILED`

Severity: `error`

Trigger: extern call cannot be linked/resolved to a callable foreign symbol.

Message contract summary:

1. `extern`.
2. `linkage`.
3. `failed`.

Data contract summary:

1. `symbol`.
2. `abi`.


### `RUNTIME_ABORT`

Severity: `error`

Trigger: unrecoverable runtime termination via:

1. explicit runtime-abort path (for example `std::test::abort!`), or
2. required runtime-fatal boundary in baseline semantics (for example an unobserved child failure at `std::concurrency` scope exit).

Invalid `par_*` worker counts are not runtime-abort triggers in v0.1; they fail at the `PositiveNat` conversion or contextual literal boundary with `DOMAIN_INVALID_VALUE`.

Message contract summary:

1. `abort`.

Data contract summary:

1. `source`.


### `GRAD_ARGNUM_OUT_OF_RANGE`

Severity: `error`

Trigger: `grad`/`value_and_grad` is called with `argnum` outside callable argument range.

Message contract summary:

1. `argnum`.
2. `range`.

Data contract summary:

1. `argnum`.


### `GRAD_NON_TENSOR_ARG`

Severity: `error`

Trigger: differentiated argument selected by `argnum` is not a tensor.

Message contract summary:

1. `grad`.
2. `non-tensor`.


### `GRAD_NON_FLOAT_DTYPE`

Severity: `error`

Trigger: differentiated tensor argument has non-floating dtype.

Message contract summary:

1. `grad`.
2. `non-float`.
3. `dtype`.


### `GRAD_PRIMAL_NOT_SCALAR`

Severity: `error`

Trigger: primal output of function passed to `grad` is not scalar in v0.1 scalar-gradient mode.

Message contract summary:

1. `scalar`.
2. `primal`.


### `GRAD_LOWERING_FAILED`

Severity: `error`

Trigger: function cannot form the pure canonical Core Tensor IR path required
for AD, including because it requires runtime tensor-value-dependent control
flow, recursion, or a Kernel IR-only construct.

Message contract summary:

1. `grad`.
2. `lowering`.
3. `failed`.


### `GRAD_UNSUPPORTED_PRIMITIVE`

Severity: `error`

Trigger: Core lowering succeeds, but the canonical AD-support graph contains a
source-active operand edge outside the closed v0.1 support table, supplies an
active value to a required-passive auxiliary slot, or attempts higher-order
differentiation of a generated gradient callable.

Message contract summary:

1. `grad`.
2. `unsupported`.
3. `primitive`.


### `GRAD_RESULT_SHAPE_MISMATCH`

Severity: `error`

Non-baseline reservation: a post-v0.1 custom derivative rule returns a gradient
whose shape differs from the differentiated primal argument shape. v0.1 exposes
no source-level custom derivative registration path.

Message contract summary:

1. `grad`.
2. `shape`.
3. `mismatch`.


### `GRAD_CUSTOM_KERNEL_NO_VJP`

Severity: `error`

Non-baseline reservation: a post-v0.1 typed custom operation is used in a
differentiable path without an explicit VJP rule. v0.1 exposes no public custom
operation or kernel registration path.

Message contract summary:

1. `custom`.
2. `kernel`.
3. `vjp`.


### `ITEM_REQUIRES_RANK0`

Severity: `error`

Trigger: `ReadableTensor.item()` is called on a `Tensor` or `TensorView` whose
rank is not 0.

Message contract summary:

1. `item`.
2. `rank`.
3. `0`.

Data contract summary:

1. `rank`.
