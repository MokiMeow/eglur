# Eglur Axes Specification

Status: Normative v0.1.0 released.

Scope: Normative for v0.1 unless this file or a section states otherwise.

## Normative Keywords

The key words `MUST`, `MUST NOT`, `SHOULD`, `SHOULD NOT`, and `MAY` are to be interpreted as described in RFC 2119 and RFC 8174.


## Scope

This document defines normative axis semantics for Eglur tensor operations.

It complements:

1. [`SPEC.md`](SPEC.md) (core language invariants and tensor transform model).
2. [`STDLIB.md`](STDLIB.md) (`std::tensor`).
3. [`ERRORS.md`](ERRORS.md) (runtime and stdlib diagnostics).

If this document conflicts with an example elsewhere, this document takes precedence for axis behavior.


## Axis Data Model

### Core Types

Axis metadata uses an overlay model:

```text
AxisIndex
AxisName
AxisSel = AxisIndex | AxisName
NameOverlay = Map[AxisIndex, AxisName]
```

Definitions:

1. `AxisIndex` is positional and addresses a concrete dimension.
2. `AxisName` is semantic and labels dimensions by meaning.
3. `NameOverlay` MAY be empty.
4. `AxisIndex` and `AxisName` implement `std::prelude::Eq` and `std::prelude::Hash` with the exact positional-index and case-sensitive-name identities defined by `stdlib/tensor.md`.


### Invariants

For tensor `t` with `rank = t.rank()` and `ov = t.names()`:

1. Every key `i` in `ov` MUST satisfy `i.to_nat() < rank`.
2. Values in `ov` MUST be unique.
3. Name comparison MUST be exact and case-sensitive.

Violations MUST return `Err(AxisError)` and emit diagnostics required by `ERRORS.md`.


### Shape Independence

Overlay metadata does not replace shape validity checks:

1. Shape checks MUST run independently from axis-name checks.
2. If both shape and axis checks can fail, axis/metadata validation MUST take precedence and the primary failure MUST be `AxisError`.


### Empty Axis Lists and Rank-0

Empty axis lists in transform patterns denote rank-0 (`shape=()`) tensors.

Invariants:

1. A rank-0 tensor MUST have an empty overlay map.
2. Operations that produce rank-0 outputs MUST produce empty overlays for those outputs.
3. Operations that consume rank-0 inputs via empty axis lists MUST treat them as having no axis names.


## Resolution Rules

### `AxisSel` Resolution

Given tensor `t` and selector `s: AxisSel`:

1. If `s` is `AxisIndex(#i)`, resolve by positional bounds check.
2. If `s` is `AxisName('name)`, resolve via `t.names()` lookup.

Failure behavior:

1. Out-of-range index MUST return `AXIS_OUT_OF_RANGE`.
2. Missing name MUST return `AXIS_NAME_NOT_FOUND`.


### Name Mutation APIs

`with_names`, `rename`, and bang forms MUST preserve data-buffer semantics.

They mutate or return metadata only, except for implementation-required header COW.


## Operation-wise Overlay Semantics

Notation:

1. `ov` is input overlay (`AxisIndex -> AxisName`).
2. `drop(i)` removes key `i`.
3. `shift_left_after(i)` decrements keys greater than `i`.

Axis-pack baseline (`...` / `...name`) for transform families:

1. A pack represents an ordered concrete-axis subsequence.
2. Overlay propagation for packs is axis-wise and order-preserving unless a transform explicitly reorders packs.
3. When packs are reordered (for example `...p1 ...p2 -> ...p2 ...p1`), overlay indices MUST follow the same concrete-axis reorder.


### Overlay APIs

`ReadableTensor.names()` MUST return the current carrier overlay unchanged.

`with_names(updates)` rules:

1. `updates` is an atomic patch over the current overlay, not a replacement overlay.
2. Existing entries whose indices are absent from `updates` MUST be preserved.
3. An update for an existing index MUST replace that index's prior name.
4. An empty update map MUST be a no-op.
5. Every update key MUST be bounds-checked before candidate-overlay construction. If more than one key is out of range, the lowest `AxisIndex` is primary (`AXIS_OUT_OF_RANGE`).
6. All updates MUST be applied simultaneously, then the final candidate overlay MUST be checked for unique names. Valid name swaps MUST NOT fail because of a transient intermediate duplicate.
7. If the final candidate contains more than one duplicated name, scan axes in increasing `AxisIndex` order and report the name whose second occurrence is encountered first (`AXIS_DUPLICATE_NAME`).
8. Bounds failure takes precedence over final-overlay duplicate-name failure.
9. An update map cannot remove an overlay entry. v0.1 defines no direct `without_names`/`clear_names` surface.

`rename(map)` rules:

1. Every source name MUST exist.
2. Resulting names MUST remain unique.


### `view{...}`

`view` is metadata+strides reinterpretation over one retained base-buffer snapshot.
The shared base establishes zero-copy construction, not write-through aliasing;
retention and mutation isolation follow `language/tensor-model.md`.

Overlay rules:

1. A named source physical axis MUST preserve its name when all coordinate
   branches of one output physical axis have that one physical-axis origin and,
   across the complete transform, that source origin contributes to no other
   output physical axis.
2. Mutually exclusive branch copies introduced by an admitted distributive or
   factoring proof retain one physical-axis origin and therefore do not by
   themselves cause a name drop.
3. A merge across physical axes, a split into several output physical axes, or
   a concatenation with several physical-axis origins MUST drop the affected
   names by default.
4. When one or more names are dropped, implementation SHOULD emit
   `AXIS_NAMES_DROPPED`.
5. Multi-output `view` computes the one-source-to-one-output condition across
   the complete transform before applying it to each output.


### `rearrange{...}`

`rearrange` materializes outputs but follows the logical coordinate mapping in
`language/tensor-model.md`, which also governs `view`.

Overlay rules:

1. `rearrange` MUST apply the same physical-axis-origin rules as `view`,
   regardless of whether its coordinate map materializes storage.
2. In particular, an admitted distributive or factoring proof MUST preserve a
   carried name when one named source physical axis maps to exactly one output
   physical axis. Merge, split, and multi-origin concatenation still drop names
   under the rules above.
3. Name ownership for `+` segments and product factors follows the deterministic
   coordinate and provenance rules from `language/tensor-model.md`.
4. A multi-input segment or product that crosses the structural operand boundary
   defined there MUST fail with `SEGMENT_STRADDLE`.
5. A schema outside the closed structural coordinate relation defined there
   MUST fail with `AXIS_MAPPING_UNSUPPORTED`. That relation admits only the
   written-tree-directed distributive and factoring proofs defined there; equal
   extents or numel do not authorize broader algebraic inference.
6. Name drops SHOULD emit `AXIS_NAMES_DROPPED`.
7. Axis-pack mappings (`...` / `...name`) are expanded as ordered axis subsequences:
   1. each concrete carried axis in the matched subsequence preserves/drop behavior independently under Rules 1-6,
   2. reordering named packs reorders overlay indices by the same concrete-axis subsequence movement.


### `inflate{...}`

`inflate` explicitly expands elements and MAY introduce zero-sized axes.

Overlay rules:

1. Carried axes preserve names.
2. Newly introduced repeat axes are unnamed by default.
3. Expanded merged axes are unnamed by default unless a 1:1 carry can be proven.
4. Zero-sized introduced axes are unnamed.


### `reduce{...}`

`reduce` eliminates axes present only on the left side.

Overlay rules:

1. Reduced axes MUST drop names.
2. Kept axes preserve names when they remain 1:1 carries.
3. Reduced-axis order MUST follow left-hand-side axis order.


### Primitive Ops (`std::tensor::prim`)

Elementwise (`add/sub/mul/div/max/min/where` and similar):

1. Shape alignment MUST use the trailing-axis baseline broadcasting algebra in
   `language/tensor-model.md`.
2. Axis names MUST NOT participate in shape compatibility or select a different
   positional alignment.
3. Overlay propagation runs after shape normalization. An input overlay key
   shifts right by the number of logically prepended axes for that input.
4. Default overlay propagation MUST be left-biased over the aligned operands.
5. Name conflicts MUST NOT fail by default.

Primitive reductions (`sum/max/mean`):

1. Axis selectors resolve left-to-right.
2. `keepdims=false`: reduced axes removed and remaining indices shifted.
3. `keepdims=true`: reduced axes remain at their original indices as size-1 axes, and
   their names MUST be preserved.
4. An empty `Vec[AxisSel]` selects no axes. The owning result MUST preserve every
   input axis and name at the same index; `keepdims` has no effect.
5. Zero-length selected axes do not change these overlay rules. Empty-domain
   values and failures follow `stdlib/conventions.md`.

Movement:

1. Every successful primitive movement MUST return a zero-copy `TensorView` over
   the input tensor's retained base-buffer snapshot. Axis metadata propagation is
   part of that view mapping and MUST NOT trigger implicit materialization.
2. `permute` MUST remap overlay indices by the same permutation.
3. `unsqueeze(x, #i)` MUST insert one unnamed axis at `#i`; every input overlay key
   less than `#i` remains unchanged and every key greater than or equal to `#i`
   shifts right by one.
4. `squeeze(x, #i)` MUST remove the size-1 axis at `#i` and its name, if present;
   every later overlay key shifts left by one.
5. `slice` with the range-only `IndexItem::All`/`IndexItem::Slice` subset MUST
   preserve rank, axis order, overlay keys, and overlay names unchanged.
6. `reshape(x, new_shape)` MUST preserve the complete input overlay unchanged when
   `new_shape` is exactly equal to `shape(x)`.
7. Every other successful `reshape` MUST produce an empty overlay. It MUST NOT infer
   partial axis correspondence from equal dimensions, products, singleton insertion
   or removal, or unchanged rank.
8. Rule 7 is intentional because primitive `reshape` carries no source-to-target
   axis mapping. Callers that require semantic name preservation MUST use a
   mapping-explicit operation such as `unsqueeze`, `squeeze`, `permute`, `view{...}`,
   or `rearrange{...}`.
9. A shape-changing `reshape` with a non-empty input overlay MAY emit
   `AXIS_NAMES_DROPPED` under the optional emit-if-present policy.
10. For successful `expand(x, new_shape)`, let
   `k = new_shape.len() - shape(x).len()`. Every input overlay entry
   `#i -> name` MUST become `#(i + k) -> name`.
11. The `k` logically prepended leading axes MUST be unnamed.
12. An aligned source axis MUST preserve its name when its size is unchanged and
    when source size `1` expands to any permitted target size, including `0`.


### `contract{...}`

`contract` uses atomic labels only and fixed multiply+sum semantics.
Every fresh contract schema uses relaxed-name mode with an empty alias map.
Name-mode and alias modifiers apply only to the schema value they return; a
nested, sibling, or later fresh contract expression does not inherit them.

Overlay rules:

1. Unless a more specific rule below applies, output label names are inherited from corresponding surviving input labels.
2. Before repeated-label diagonal normalization, every source-axis occurrence
   participates independently in strict-name comparison.
3. In strict-name mode, `OpSchemaN.with_aliases` canonicalizes observed input
   names before comparison. Multiple external names MAY canonicalize to one
   repeated schema label. The diagonal intermediate carries that canonical name
   at the group's first source-axis position. If the label survives on the
   right-hand side, the final output axis carries the canonical schema label at
   its right-hand-side position.
4. In strict-name mode, incompatible bindings MUST fail with `AXIS_MISMATCH`.
5. In relaxed-name mode, implementation MAY drop conflicting names and `with_aliases` does not affect compatibility checking.
6. Empty output axis lists MUST produce rank-0 outputs with empty overlays.


### `einop{...}`

In v0.1, `einop` combines atomic/product-only structural mapping and fixed
multiply+sum elimination. A tagged-sum `+` remains parse-valid but is rejected
with `AXIS_MAPPING_UNSUPPORTED` before mapping or lowering.
Multi-output `einop` returns tuple elements in right-hand-side output order.
Identifier identity, fresh literal/output axes, pack expansion, diagonal
normalization, and output-independent lowering are defined only by the
[authoritative language algorithm](language/tensor-model.md#einop-normalization-and-lowering).

Overlay rules:

1. Output labels that correspond to unique surviving named axes MAY preserve names.
2. Labels introduced via merge/split/concat or ambiguous provenance MUST be unnamed by default.
3. Name drops SHOULD emit `AXIS_NAMES_DROPPED`.


### Indexing (`get_at` / `set_at` / `set_at!`)

For `IndexSpec` operations, arity completion and `IndexItem::Rest` expansion run
before overlay transformation under `stdlib/tensor.md`. `Rest` therefore emits
no axis directly.

1. `IndexItem::All`/`IndexItem::Slice` preserve the consumed source-axis name.
2. `IndexItem::At` removes the consumed source axis and drops its name.
3. `IndexItem::NewAxis` inserts a size-1 unnamed axis.
4. All `IndexItem::Take` coordinate tensors broadcast to one common index block
   inserted at the first `Take` position, even when `Take` items are separated.
5. A common index block preserves a source-axis name only for exactly one
   `Take` whose common index shape has rank 1. In every other case all common
   index axes are unnamed. Coordinate-tensor names never propagate.
6. `set_at` and `set_at!` do not change overlay shape/name layout of the
   destination tensor; value overlays do not participate in update-shape
   compatibility.
7. `set_at` is pure generalized scatter and `set_at!` is bang generalized
   scatter.


### `map_axis`

For canonical `map_axis`:

1. For each mapped input axis, per-step slicing removes the mapped axis from
   that step input while preserving the position's concrete carrier.
2. A `TensorView` step is a zero-copy affine view; a `Tensor` step has owning
   value semantics.
3. Removing a mapped axis MUST drop the removed axis name and shift following overlay indices left by 1.
4. The first successful `Tensor` or `TensorView` callback result establishes the baseline result overlay.
5. Every later successful callback result MUST have exactly the same `AxisIndex -> AxisName` bindings as the baseline overlay. `Map` insertion order is not part of this comparison; the earliest mismatch MUST fail with `MAP_AXIS_RESULT_OVERLAY_MISMATCH`.
6. Stacking mapped results inserts a new axis at `out_axis` and produces a
   materialized `Tensor`.
7. Insertion at `out_axis` MUST shift existing overlay indices greater than or equal to `out_axis` right by 1.
8. If `out_name` is provided, the inserted axis receives that name.
9. `out_name` MUST be rejected with `MAP_AXIS_OUT_NAME_DUPLICATE` when it would duplicate an existing output axis name.


## Diagnostics Mapping

This section maps axis semantics to required diagnostic codes in `ERRORS.md`.

Structured assertion semantics are defined centrally in `CONFORMANCE.md` and `errors/conformance-map.md`.

Axis-specific suites MUST follow that central contract and MUST NOT redefine a parallel diagnostic-assertion format in this document.

### Structured Assertion Contract (Normative Reference)

For axis diagnostics asserted in conformance suites:

1. Primary assertions MUST use structured diagnostics (`expect.value_diagnostics`, `expect.compile_error`, or `expect.runtime_error`) per `CONFORMANCE.md`.
2. Each structured diagnostic assertion object MUST include:
   1. `code`,
   2. `message_contains`,
   3. `data_keys` (MAY be empty).
3. Structured diagnostic assertion objects MAY additionally include `data_equals`:
   1. keys MUST be a subset of `data_keys`,
   2. values MUST be `Str`, signed `i64`, or `Bool` YAML scalars,
   3. matching is exact scalar kind/value equality against emitted diagnostic `data`.
4. `message_contains_any` MUST be present when the machine contract defines a
   one-of message group.
5. Required axis suites MUST NOT use `help_contains`, `related_contains`, or
   top-level `diagnostic_contains`.

### Required Error Codes

1. `AXIS_OUT_OF_RANGE`
2. `AXIS_NAME_NOT_FOUND`
3. `AXIS_DUPLICATE_NAME`
4. `AXIS_MISMATCH`
5. `AXIS_MAPPING_UNSUPPORTED`
6. `SEGMENT_STRADDLE`
7. `MAP_AXIS_RESULT_OVERLAY_MISMATCH`
8. `MAP_AXIS_OUT_NAME_DUPLICATE`

### Non-baseline Conditional Error Codes

1. `AXIS_ALIASED_HEADER` (strict-alias extension profiles only).


### Optional Emit-if-present Warning Codes

1. `AXIS_NAMES_DROPPED`


### Optional Emit-if-present Note Codes

1. `AXIS_OVERLAY_PERMUTED`

If any optional emit-if-present code above is emitted, it MUST still satisfy the same structured assertion contract (`code` + scoped token fields + `data_keys`, plus `data_equals` when asserted) from `CONFORMANCE.md`.


## Conformance Requirements

A conforming implementation for axis support MUST satisfy:

1. Tensor axis-related tests in `../../tests/conformance/tensor/*.yaml`.
2. All normative `MUST` rules in this document.
3. Required axis diagnostics MUST be asserted through structured assertion objects defined in `CONFORMANCE.md`.
4. Axis diagnostic policy class (hard-required, optional_emit_if_present, excluded_nonbaseline) MUST match `errors/conformance-map.md`.


## Non-goals (v0.1)

The following remain deferred:

1. Name-unification calculus beyond baseline positional alignment and left-biased
   overlay propagation.
2. Complete machine-checked proof of `contract`/`einop` matching correctness.
3. Type-level axis soundness proofs.
4. Global policy for optional warning/note volume.


## Examples

### Naming and Reduction

```eglur fixture=RDX-public-axes-naming-and-reduction-example
import std::tensor::{AxisError, DTypeError, NumericError, ShapeError, ones}

fn main!() -> Res[Str, Union[AxisError, DTypeError, NumericError, ShapeError]] =
  result {
    let t = ones[f32](2,3).with_names([#0:'mol, #1:'task])?
    let u = reduce{ b c -> b }(t)?
    yield Ok(to_str(u.axis_of('mol)? == #0))
  }
```


### Permute Overlay

```eglur fixture=AX-permute-remaps-overlay-indices
import std::tensor::{AxisError, ShapeError, zeros}
import std::tensor::prim::permute

fn main!() -> Res[Str, Union[AxisError, ShapeError]] =
  result {
    let t = zeros[f32](2,3,4).with_names([#0:'B, #1:'T, #2:'C])?
    let u = permute(t, [#0, #2, #1].to_vec())?
    let ok = ((u.names().len() == 3)
      && (u.axis_of('B)? == #0)
      && (u.axis_of('C)? == #1)
      && (u.axis_of('T)? == #2))
    yield Ok(to_str(ok))
  }
```


### Rearrange Name Drop

```eglur fixture=XFM-public-axes-rearrange-name-drop-example
import std::tensor::{AxisError, TransformError, zeros}

fn main!() -> Res[Str, Union[AxisError, TransformError]] =
  result {
    let t = zeros[f32](2,3).with_names([#0:'mol, #1:'task])?
    let u = rearrange{ b c -> (b*c) }(t)?
    yield Ok(to_str(u.names().len() == 0))
  }
```
