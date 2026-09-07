# Tensor Transform Model

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Pattern Core and Dim Solving

Tensor transform literals (`view{...}`, `rearrange{...}`, `inflate{...}`, `reduce{...}`, `contract{...}`, `einop{...}`) share a pattern core:

1. `<operands> -> <outputs>`.
2. Left side and right side each contain one or more axis lists, separated by commas.
3. Each axis list MAY be empty or contain space-separated axis entries.
4. An empty axis list denotes rank-0 (`shape=()`).

Axis expressions MAY use integer literals, dimension variables, `+`, `*`, and parentheses except where an operation imposes stricter grammar (for example `contract`).

Axis expression precedence MUST be:

1. Parentheses.
2. `*`.
3. `+`.

`+` and `*` with the same operator class MUST associate left-to-right.

### Axes as First-class Tensor Information

Axis semantics MUST be representable as executable metadata.

Runtime axis naming support is REQUIRED in the baseline tensor model.

Axis metadata MUST be modeled as `NameOverlay = Map[AxisIndex, AxisName]`.

`AxisIndex` and `AxisName` MUST be distinct canonical `std::tensor` types.
`AxisIndex` identifies positional dimensions and `AxisName` provides semantic
labels. Their language-owned literal construction identity is defined in
`syntax.md` and `implementation-boundary.md`.

Type-level axes are OPTIONAL non-baseline extension functionality. Their absence
does not make the runtime tensor source layer optional.

Detailed propagation rules are defined in [the axes specification](../axes.md).

Axis-list entries in transform literals (`view`, `rearrange`, `inflate`, `reduce`, `einop`) are one of:

1. an `AxisExpr` (single concrete axis), or
2. an axis-pack token:
   1. anonymous pack: `...`,
   2. named pack: `...name`.

Axis-pack rules in v0.1:

1. Any pack matches a sequence of zero or more concrete axes.
2. Anonymous pack `...` is allowed for simple one-pack matching, but MUST appear at most once per transform literal.
3. If more than one variadic capture is required, named packs MUST be used (`...p1`, `...p2`, ...).
4. Output-side named packs MUST be bound on at least one input-side occurrence; unbound output named packs MUST fail with `AXIS_PACK_UNBOUND`.
5. Repeated occurrences of the same named pack MUST bind to identical axis sequences.

Implementations MUST resolve all dimension variables to `Nat` values before
execution, using the closed baseline procedure in `../stdlib/tensor.md`.
The v0.1 procedure is intentionally complete only for ground equations,
fixed-point univariate equations over non-negative `+`/`*` expressions, and the
listed closed ambiguity certificates. It MUST NOT invoke a general
multivariate integer, Diophantine, factorization, SMT, or backend solver.

Explicit `.with_sizes(...)` bindings MUST be `Nat` values.
Signed runtime values MUST be converted with `Nat.from_i64` before map
construction.

The eligible `.with_sizes(...)` key set is exactly the scalar
dimension-variable identifiers that occur anywhere in the receiver schema's
input or output axis expressions. Named and anonymous axis packs are not
eligible keys. A binding map MAY contain any subset of eligible keys; omitted
variables remain available to inference, while supplied variables act as
consistency constraints even when inference could recover them.

An ineligible key MUST fail with `WITH_SIZES_UNKNOWN_KEY` before dimension
solving. Const-evaluable offending keys fail at compile time; otherwise schema
invocation returns `Err(...)` through the operation's existing error family.
When multiple offending keys are present, the first key in `Map` insertion
order is reported.

The baseline solver has four semantic outcomes:

1. no valid solution proven by the closed procedure:
   `INCONSISTENT_DIMS`,
2. exactly one complete solution: success,
3. at least two valid solutions, including a free-variable family:
   `AMBIGUOUS_DIMS`,
4. unresolved multivariate constraints outside the closed procedure:
   `DIM_BINDING_REQUIRED`.

`DIM_BINDING_REQUIRED` is not ambiguity. It requires enough explicit
`.with_sizes(...)` bindings to reduce every unresolved equation component to
the baseline ground or univariate fragment. Implementations MUST NOT accept a
non-unique solve even when downstream output shape or value would be unique.

Pack partitioning participates in the same outcome classification. A complete
solution includes both the dimension-variable mapping and every named-pack
binding. Its candidate space is finite because each pack matches a subsequence
of a concrete operand rank. No valid partition is `INCONSISTENT_DIMS`; two valid
complete partitions are `AMBIGUOUS_DIMS`; and a partition whose dimension
equations remain outside the baseline fragment contributes
`DIM_BINDING_REQUIRED`. More than one anonymous pack occurrence still fails
earlier with `AXIS_PACK_ANON_DUPLICATE`.

### Boundary with General Type System

This chapter specifies tensor-constraint behavior and is paired with
`type-system.md` type-check rules and the one-way layering contract in
`implementation-boundary.md`.

Rules:

1. Tensor constraints are not independent type-level predicates in v0.1 and MUST NOT alter trait selection/coherence resolution.
2. Tensor axis metadata and runtime shape values are runtime properties, not nominal type identity components.
3. Tensor operations MUST expose runtime-dependent validation failures through fallible return channels (`Res`/`Opt` contracts) rather than compile-time rejection.
4. Statically decidable tensor-constraint failures MUST be rejected at compile time using required tensor diagnostic codes.
5. Backend-specific lowering, fusion, and kernel planning MUST NOT change whether a program is well-typed.


### Baseline Broadcasting Algebra

A tensor shape is a finite ordered sequence of `Nat` dimensions. Its rank is the
sequence length; the empty sequence `()` is rank 0.

For one or more operand shapes, baseline broadcasting MUST compute one common
shape as follows:

1. Let `R` be the maximum operand rank.
2. Normalize each operand shape to rank `R` by logically prepending size-1 axes.
   This normalization does not mutate the operand or its axis-name overlay.
3. At each normalized output axis, discard every dimension equal to `1`:
   1. if no dimensions remain, the result dimension is `1`,
   2. if all remaining dimensions are equal, that common dimension is the result,
   3. otherwise the shapes are incompatible.
4. Compatibility and the result shape MUST be independent of operand grouping.
   N-ary consumers such as `where` and multi-coordinate `Take` MUST apply this
   rule to all operand shapes directly, not through an observable pairwise order.
5. On incompatibility, normalized output axes are examined in increasing
   `AxisIndex` order. The lowest incompatible output axis is primary. If an error
   reports operand positions, operands at that axis are examined left-to-right
   and the earliest later non-1 dimension that differs from the first prior
   non-1 dimension is primary.

Consequences:

1. Shapes align from their trailing axes. For example, `(2, 3)` and `(3)` produce
   `(2, 3)`, while `(2, 3)` and `(2)` are incompatible.
2. A rank-0 tensor broadcasts to any tensor shape and remains a tensor value; no
   implicit `Tensor[T] -> T` scalar conversion occurs.
3. Dimension `0` is not a wildcard. It is compatible with `0` and `1`, producing
   `0`, and is incompatible with every dimension greater than `1`.
4. A broadcast result containing a zero-sized axis has no logical elements.
   Elementwise computation for that result MUST invoke the scalar operation zero
   times.
5. Axis names do not participate in shape compatibility. Name-overlay propagation
   runs only after positional shape alignment under `../axes.md`.

For a result coordinate, an operand coordinate uses `0` at every normalized
operand axis of size `1` and otherwise uses the result coordinate at that axis.
Logically prepended axes are omitted again when indexing the original operand.

Directed expansion from source shape `S` to target shape `T` MUST succeed exactly
when:

1. `rank(S) <= rank(T)`,
2. `S` is normalized to `rank(T)` by logically prepending size-1 axes, and
3. every normalized source dimension is either equal to its target dimension or
   equal to `1`.

The target shape is the result exactly. A source dimension `1` MAY expand to
target dimension `0`; a source dimension `0` cannot expand to a nonzero target
dimension. Directed expansion is not a symmetric common-shape operation.

Statically decidable incompatibility follows the compile-time boundary above.
Runtime-dependent incompatibility MUST use the consuming operation's declared
shape-related error channel. This algebra defines logical shape and coordinate
mapping only; carrier, allocation, and materialization guarantees remain
operation-specific.


### `Tensor` and `TensorView` Value Semantics

`Tensor[T]` and `TensorView[T]` are ownership-free values and follow the ordinary
by-value call and assignment rules in `calls-references.md`.

Rules:

1. `TensorView[T]` is a read-only affine view over a retained snapshot of one base
   buffer and its logical tensor metadata.
2. A view retains that snapshot independently of the source binding. It MUST remain
   valid after the source binding leaves scope, is reassigned, or is otherwise no
   longer reachable.
3. Assignment, argument passing, and return of a view copy the same logical snapshot.
   Implementations MAY share its retained storage and metadata.
4. "Same base buffer" describes zero-copy view construction and snapshot identity;
   it does not create observable write-through aliasing.
5. Mutation of a source `Tensor`, a copied `Tensor`, or a tensor produced by
   `TensorView.materialize()` MUST NOT change an existing view or any copy of that
   view. Mutation through one of those values MUST isolate storage and metadata first
   when required.
6. Implementations MAY satisfy Rule 5 with copy-on-write, eager copying, ownership
   transfer, or another representation strategy, provided the required value
   semantics remain observable.
7. `TensorView[T]` has no mutable receiver surface in v0.1 and is not assignable to
   `&mut Tensor[T]`. The language defines no implicit `TensorView[T] -> Tensor[T]` or
   `Tensor[T] -> TensorView[T]` conversion.
8. `TensorView.materialize()` is the explicit view-to-owning-tensor boundary. It
   preserves the view's logical values, dtype, device, shape, axis order, and name
   overlay at the instant of the call, and its result is mutation-independent from
   the view and its source.

The closed standard-library carrier capabilities classify these value semantics
without changing them:

1. `ReadableTensor` exposes logical read/query operations and the associated
   element type. It does not grant mutation or imply materialization.
2. `AffineTensor` is a methodless marker for carriers that can supply the
   retained logical snapshot required by zero-copy affine view paths.
3. `Tensor[T]` and `TensorView[T]` implement both capabilities. The marker does
   not mean that every requested transform is affine; operation-specific rules
   still decide whether a view path succeeds.
4. Capability bounds do not insert conversion, borrowing, materialization, or
   carrier erasure. A generic parameter remains its concrete carrier.
5. The exact public implementor, visibility, and callable-identity contract is
   declared by `../stdlib/tensor.md`.


### Deterministic Axis-expression Coordinate Mapping

After dimension solving has produced one complete solution, every `AxisExpr`
MUST denote an ordered finite coordinate domain. This logical order is
independent of physical buffer layout and carrier strides.

For a resolved expression `E`, write `extent(E)` for its mathematical
non-negative extent. Its coordinate encoding is defined recursively:

1. An identifier or integer-literal occurrence with resolved extent `d` denotes
   coordinates `0 <= i < d`.
2. `A + B` is the ordered disjoint union of `A` then `B`:
   1. `A` coordinate `i` encodes as `i`,
   2. `B` coordinate `j` encodes as `extent(A) + j`,
   3. decoding `q` selects `A` when `q < extent(A)` and otherwise selects `B`
      with coordinate `q - extent(A)`.
3. `A * B` is the ordered Cartesian product with the right operand varying
   fastest:
   1. `(i, j)` encodes as `i * extent(B) + j`,
   2. for a non-empty product, `q` decodes as
      `(q / extent(B), q % extent(B))`,
   3. nested products apply these rules recursively according to the parsed
      expression tree.
4. Equivalently, for a product with `k >= 2` resolved factor extents
   `d_0, ..., d_(k-1)`, coordinates `i_0, ..., i_(k-1)` encode in logical
   row-major order as:

   ```text
   q = i_0 * (d_1 * ... * d_(k-1))
     + i_1 * (d_2 * ... * d_(k-1))
     + ...
     + i_(k-2) * d_(k-1)
     + i_(k-1)
   ```

   The rightmost factor has unit stride.
5. Repeated variable occurrences are separate coordinate positions that share
   one resolved extent. For example, `(h * h)` decodes to two coordinates in
   `[0, h)`, not one diagonal coordinate. A structural mapping that carries
   indistinguishable repeated occurrences pairs them in stable left-to-right
   occurrence order, except for mutually exclusive branch copies whose lineage
   is established by a distributive proof below.
6. Parentheses affect the recursive sum/product structure but do not introduce
   a coordinate position of their own.

The encoder and decoder above MUST be mutual inverses on every non-empty
resolved expression domain.

v0.1 constructs structural coordinate transport from a closed set of written
operations:

1. carry one source occurrence to one destination occurrence,
2. associate, split, merge, or permute product factors,
3. associate, split, concatenate, or permute explicitly written sum segments,
4. expand or contract axis-list boundaries under the selected transform
   family's input/output rules,
5. apply the written-tree-directed distributive or factoring proof rules below,
   and
6. duplicate or eliminate coordinates only when the transform family declares
   that behavior, such as `inflate`, `reduce`, or `einop`.

Same-operator association does not change this relation. For the distributive
rules, source leaf occurrences have distinct identities even when their labels
have the same spelling. Write `D(S, T)` when the two written `AxisExpr` trees
admit structural transport. In addition to the direct operations above, the
least such relation contains these recursive rules:

1. Left distribution:
   `D(A * C, X)` and `D(B * C, Y)` establish
   `D((A + B) * C, X + Y)`.
2. Right distribution:
   `D(A * B, X)` and `D(A * C, Y)` establish
   `D(A * (B + C), X + Y)`.
3. Left factoring:
   `D(X, A * C)` and `D(Y, B * C)` establish
   `D(X + Y, (A + B) * C)`.
4. Right factoring:
   `D(X, A * B)` and `D(Y, A * C)` establish
   `D(X + Y, A * (B + C))`.
5. A proof MAY apply these rules recursively to the corresponding written
   subtrees and MAY use same-operator association to align those subtrees.
6. A repeated factor introduced by a distributive premise retains the identity
   of the one source subtree occurrence. Its destination copies occur only in
   mutually exclusive `+` branches. They do not denote duplicated coordinates
   and MUST NOT be paired as independent same-spelled occurrences.
7. Operand-root and physical-axis-origin tags are not premises of `D` and do not
   affect whether a structural proof exists or which complete proof is selected.
   The selected proof transports those tags unchanged. In particular, inverse
   factoring does not merge the provenance of branch factors merely because
   they correspond to one destination factor.

This is a written-tree-directed structural proof, not general algebraic
equivalence. Expansion is directed by the written target sum; inverse factoring
is directed by the written source sum.

1. Every distributive step MUST consume one sum node from the written expanded
   side. The compact side is either one written product subtree or one local
   product obligation produced by its parent proof rule. Implementations MUST
   produce no child product obligations other than the two shown by Rules 1-4;
   the written sum node supplies the finite branch structure they MUST prove.
2. Implementations MUST NOT construct or search a fully distributed normal form
   whose branch structure is absent from both written trees.
3. Implementations MUST NOT use commutativity, idempotence, cancellation,
   numeric equality of resolved extents, or equality of global numel to invent a
   proof. `+` and `*` remain ordered operations.
4. If no proof exists in this closed relation, the schema MUST fail with
   `AXIS_MAPPING_UNSUPPORTED` before dimension solving.

Proof selection and its induced coordinate map MUST be deterministic:

1. A complete direct structural match is selected before a distributive or
   factoring rule at the same subtree pair.
2. When both left and right distribution can completely prove the same written
   subtree pair, left distribution is selected. This preserves the product
   convention that the left factor supplies the outer coordinate and the right
   factor varies fastest.
3. Left factoring has the same priority over right factoring for the inverse
   subtree pair.
4. A lower-priority rule is considered only when the higher-priority rule cannot
   complete that subtree proof. Once a complete proof is selected, an
   implementation MUST NOT search for an alternative map.

An explicitly written sum-segment permutation MAY compose with the selected
distributive or factoring proof:

1. The selected proof first establishes one canonical sequence of expanded
   branch obligations and their source-occurrence lineage.
2. The written expanded side MUST have the same finite branch count and admit a
   complete one-to-one match from those canonical obligations using only the
   direct structural operations admitted above. Target branch order defines
   output order for distribution; source branch order defines input order for
   inverse factoring.
3. Matching MUST use occurrence lineage and written structure, never resolved
   extent or numel equality. If several complete matches remain because branches
   are indistinguishable, implementations MUST choose the lexicographically
   smallest sequence of canonical branch indices in written order.
4. This matching phase MAY use deterministic bipartite matching or an equivalent
   finite algorithm. It MUST NOT enumerate branch permutations or reconsider the
   distributive/factoring skeleton selected by the priority rules above.

The coordinate transport for one admitted distributive step is the canonical
bijection between tagged sum coordinates and product coordinates:

1. `(A + B) * C -> (A * C) + (B * C)` maps
   `(left(i), k)` to `left((i, k))` and `(right(j), k)` to
   `right((j, k))`.
2. `A * (B + C) -> (A * B) + (A * C)` maps
   `(i, left(j))` to `left((i, j))` and `(i, right(k))` to
   `right((i, k))`.
3. Factoring uses the inverse map, and nested proofs compose these maps in proof
   order.
4. Empty branches use the same unique empty-domain maps as other zero-extent
   expressions. No branch is visited and no division or remainder is performed
   for an empty domain.

A transform family's explicit non-overlapping multi-output partition remains a
separate written operation. Repeated surrounding axes in separate output lists
identify the slices declared by that partition; they do not by themselves
supply a one-`AxisExpr` distributive proof.

For a mapping admitted by that closed relation, transform mapping composes the
coordinate encoders and decoders as follows:

1. Decode each physical input-axis coordinate through its left-hand-side
   `AxisExpr`.
2. Apply the operation's declared carry, permutation, concatenation,
   distributive branch transport, duplication, or elimination behavior to those
   logical coordinates.
3. Encode each physical output-axis coordinate through its right-hand-side
   `AxisExpr`.

Consequences:

1. Splitting `(h * w)` into `h w` maps flat coordinate `q` to
   `(q / w, q % w)`.
2. Merging `h w` into `(h * w)` maps `(i_h, i_w)` to `i_h * w + i_w`.
3. Reordering factors is observable. Merging `h w` into `(w * h)` maps to
   `i_w * h + i_h`.
4. For `inflate{ i -> (i * r) }`, the output-only `r` coordinate selects a
   duplicate of input coordinate `i`; `r` varies fastest.
5. Distribution and factoring preserve element count without duplicating or
   dropping values, but they need not preserve flat coordinate order. With
   `a=2`, `b=1`, and `c=2`,
   `rearrange{ a*(b+c) -> (a*b)+(a*c) }` maps input values
   `[v0, v1, v2, v3, v4, v5]` to `[v0, v3, v1, v2, v4, v5]`.
   An explicit branch permutation in
   `rearrange{ a*(b+c) -> (a*c)+(a*b) }` instead produces
   `[v1, v2, v4, v5, v0, v3]`.
6. `view` uses this logical mapping only when the result is representable as
   its required affine zero-copy view. Physical strides MUST NOT change the
   logical result; an unrepresentable mapping, including a non-affine admitted
   distributive map, fails with `NOT_A_VIEW`.
7. Materializing operations, including `rearrange`, `inflate`, and `einop`,
   MUST write values in the logical output order defined here regardless of
   source or destination physical layout.
8. A non-affine admitted distributive map MUST reach lowering as an explicit
   output-to-source coordinate map or equivalent fixed copy plan. Core Tensor IR
   MAY express that plan with `IOTA`, index arithmetic, `GATHER`, and `REALIZE`;
   a backend MUST NOT infer a different branch order from shape equality alone.

Coordinate-map validation rules:

1. Every dimension variable MUST resolve to `Nat` before map construction.
2. A consumed physical axis extent MUST equal the mathematical extent of its
   resolved expression exactly. Product inference MUST require exact
   divisibility; floor or truncating factor inference is forbidden. Failure
   uses the applicable dimension-solver outcome, including
   `INCONSISTENT_DIMS`.
3. Expression extents are evaluated as mathematical non-negative integers
   after the solver's required constant folding and zero identities. A
   nonzero resolved expression extent greater than `Nat::MAX` cannot denote a
   tensor axis and MUST fail with `INCONSISTENT_DIMS`; it MUST NOT wrap.
4. For a valid non-empty product, the complete product and every suffix stride
   fit in `Nat`. Implementations MUST use checked or wider intermediate
   arithmetic and MUST NOT expose machine-integer wrapping. Coordinate metadata
   arithmetic is not tensor element arithmetic and does not emit
   `INT_OVERFLOW`.
5. If any product factor has extent `0`, the product coordinate domain is
   empty. Its forward and inverse maps are the unique empty functions; no
   division or remainder operation is performed. Unresolved variables in a
   zero-product equation still follow the dimension solver's ambiguity rules.
6. Schema, dimension, segment, numel, and affine-map validation MUST complete
   before element traversal. An implementation MUST NOT repair an invalid map
   by truncating, wrapping, dropping, duplicating, or reordering elements.
7. The closed structural mapping relation is checked before dimension solving.
   `AXIS_MAPPING_UNSUPPORTED` therefore takes precedence over dimension,
   segment, numel, dtype, and affine-map diagnostics for the same invocation.

For multi-input structural mapping, segment ownership is definition-level and
deterministic:

1. `A + B` maps to prefix `A` then suffix `B`.
2. Concat order MUST follow input operand order.
3. Interleaving across operand boundaries is forbidden.
4. A `rearrange` product node MUST consume factors from at most one input
   operand root. A product whose factors carry provenance from multiple input
   roots MUST fail with `SEGMENT_STRADDLE`, even when global input and output
   element counts happen to match.
5. The closed structural proof MUST complete before operand-root provenance is
   validated. A written factoring proof that succeeds structurally but makes
   one destination product consume multiple input roots therefore fails with
   `SEGMENT_STRADDLE`, not `AXIS_MAPPING_UNSUPPORTED`.
6. Rule 5 does not authorize provenance to repair an unsupported tree map. When
   no proof exists in `D`, `AXIS_MAPPING_UNSUPPORTED` retains the precedence
   defined above.
7. Segment-boundary validation precedes global numel comparison.
8. Rule 4 does not prohibit `einop` output products whose labels originate in
   different operands. `einop` combines operand values through its declared
   multiply-and-sum semantics rather than treating those labels as one
   structural input segment.


### Rearrange Contributor Provenance

Every valid `rearrange` mapping carries schema-level input provenance:

1. Each left-hand-side operand root is tagged with its 0-based input position.
   A rank-0 operand's conceptual scalar segment retains that tag even though its
   axis list is empty.
2. Each source physical-axis occurrence also receives a stable physical-axis
   origin distinct from the operand-root tag. Its resolved axis-pack member,
   leaf occurrences, and symbolic segments inherit both identities. Equal label
   spelling does not merge either identity.
3. Direct carry, grouping, splitting, multiplication, pack expansion, and
   distributive/factoring transport propagate the tags of the source segments
   they consume. Mutually exclusive branch copies retain the same source tag;
   concatenation propagates their ordered union under the deterministic `+`
   rules above.
4. An output's contributor list is the stable de-duplication, in input-operand
   order, of the tags consumed by that complete output axis list. Rank-0 input
   scalar segments participate in input-operand order, and rank-0 outputs
   consume those mapped segments in right-hand-side order.
5. Runtime zero-length segments retain schema provenance; output dtype MUST NOT
   depend on whether a runtime extent is zero.
6. Every valid output mapping MUST have at least one contributor. Failure to
   construct a complete mapping is handled by the applicable schema, dimension,
   segment, or numel diagnostic before dtype validation.

Axis-name overlay propagation uses physical-axis origins rather than operand-root
tags or storage materialization:

1. Mutually exclusive branch copies produced by distributive or factoring
   transport retain the physical-axis origin of the source physical axis.
2. A named source physical axis maps 1:1 for overlay purposes only when all
   coordinate branches of one destination physical axis carry that one origin
   and that origin contributes to no other destination physical axis in the
   complete transform.
3. `view` and `rearrange` MUST preserve the name for such a 1:1 mapping. A
   physical-axis merge, split, or multi-origin concatenation is not 1:1 and
   drops affected names under the rules in `../axes.md`.


### Operation Families and Contracts

`view{...}`:

1. Its input MUST have a concrete carrier satisfying `ReadableTensor +
   AffineTensor`.
2. MUST be unary in v0.1 (`MULTI_INPUT_NOT_ALLOWED` otherwise).
3. MUST be zero-copy on success and produce `TensorView` outputs over the same retained
   base-buffer snapshot under the value-semantics rules above.
4. MUST fail with `NOT_A_VIEW` when affine view mapping is impossible.
5. MUST fail with `AXIS_MAPPING_UNSUPPORTED` when its written schema requires
   an algebraic rewrite outside the closed structural mapping relation.

`rearrange{...}`:

1. Every input MUST have a concrete carrier satisfying `ReadableTensor`.
2. MAY be multi-input and multi-output.
3. Each output's element dtype is the common canonical element type of its
   ordered contributor list under the provenance rules above, after ordinary
   call-site carrier inference and associated-projection normalization.
4. Separate outputs MAY retain different element dtypes when their contributor
   sets are disjoint or otherwise canonically homogeneous.
5. One output MUST fail at compile time with
   `REARRANGE_OUTPUT_DTYPE_MISMATCH` when its contributor element types are not
   all canonically equal. `rearrange` performs no implicit promotion or cast.
6. MUST materialize owning output tensors.
7. MUST preserve total element count (no implicit duplication or drop).
8. `numel` growth MUST fail with `NUMEL_MISMATCH_GROW`.
9. `numel` shrink MUST fail with `NUMEL_MISMATCH_SHRINK`.
10. MUST fail with `AXIS_MAPPING_UNSUPPORTED` when its written schema requires
    an algebraic rewrite outside the closed structural mapping relation.

`inflate{...}`:

1. Its input MUST have a concrete carrier satisfying `ReadableTensor`.
2. MUST perform explicit duplication and materialize outputs.
3. MUST change element count only by explicit repeat factors in the pattern.
4. Zero repeat factors are allowed and MAY yield empty outputs.
5. MUST fail with `AXIS_MAPPING_UNSUPPORTED` when the requested coordinate
   transport is not generated by its explicit carry and repeat-factor rules.

Primitive movement (`permute`, `reshape`, `unsqueeze`, `squeeze`, `expand`, and
range-only `slice`):

1. Except for the concrete `reshape` overloads, each source MUST have a concrete
   carrier satisfying `ReadableTensor + AffineTensor`. `reshape` accepts the
   distinct `Tensor[T]` and `TensorView[T]` contracts defined by `std::tensor`.
2. MUST produce `TensorView` outputs over the input carrier's retained
   base-buffer snapshot on success.
3. MUST be zero-copy and affine; element-buffer allocation, copy, and memcpy are
   forbidden during movement construction.
4. MUST NOT insert implicit materialization when a subsequent operation requires
   `Tensor`; callers use `TensorView.materialize()` explicitly.
5. MUST preserve the operation-specific logical values, shape, axis order, and
   name-overlay rules defined by `std::tensor`.

`reduce{...}`:

1. The default reducer accepts a concrete `ReadableTensor` carrier and
   materializes its owning result.
2. A custom-reducer invocation additionally requires `AffineTensor` because
   every callback phase receives a retained `TensorView` subtensor.
3. Custom reducers are allowed only via `.reduce_by(...)`.
4. Ordered phase reducers are allowed via `.reduce_by((axes_1, f_1), (axes_2, f_2), ...)`.
5. Single reducer form `.reduce_by(f)` MUST desugar to one ordered phase over all reduced axes.
6. Reduced-axis order MUST follow left-hand-side axis order.
7. For invocation carrier `C` and `N` written phases indexed `0..N-1`, custom
   reducer typing is the heterogeneous chain `T_0 = C::Elem`,
   `Fn(TensorView[T_i]) -> Res[T_(i+1), E_i]` for each phase `i`, and final
   owning result `Tensor[T_N]`.
8. Phase callbacks MUST be checked left-to-right. Each successful phase fixes
   the next phase's input dtype; no common callback input or output dtype is
   imposed across the chain.
9. Every callback successful payload type MUST inhabit the documented
   `DType[T]` family; custom reduction MUST NOT construct an unsupported tensor
   element type such as baseline `Tri`.
10. Each `E_i` is the exact normalized error argument of the specialized phase
    callback result. The invocation error type is the normalized union of every
    `E_i` and `TransformError`.
11. When `ReduceBySchema` construction and invocation form one complete
    expression, the invocation carrier fixes `T_0` before input-dependent inline
    callback parameters are checked. The callback chain is then checked
    left-to-right under Rules 8-10.
12. A stored `ReduceBySchema` initializer MUST be fully determined at its
    construction site. Invocation constraints do not cross the binding
    initializer; an underdetermined inline callback therefore fails with
    `BINDING_TYPE_UNDETERMINED`. The complete expression-local typing and
    diagnostic rules are defined by `../stdlib/tensor.md`.
13. A non-empty integer default `sum` reduction MUST use the canonical
    source-axis traversal and checked result-dtype left fold in
    `../stdlib/conventions.md`. The first overflowing fold step MUST propagate as
    `Err(NumericError)` with cause `INT_OVERFLOW`.
14. An empty default-reducer slice MUST produce the additive identity in the
    input element dtype under the empty-domain contract in
    `../stdlib/conventions.md`. This fixed identity does not apply to
    user-supplied `reduce_by` callbacks.

`contract{...}`:

1. Every operand MUST have a concrete carrier satisfying `ReadableTensor`.
2. Is fixed einsum semantics (`multiply` then `sum`) in v0.1.
3. MUST accept one or more operands (arity `1..N`).
4. MUST use atomic axis labels only (no `+`, `*`, literals, grouped expressions, or axis-pack tokens `...` / `...name`).
5. `contract` semiring customization member forms such as `.by(...)` and `.contract_by(...)` are undefined in v0.1 and MUST fail with `METHOD_NOT_FOUND`.
6. A `contract` schema MUST contain exactly one right-hand-side axis list.
7. Repeated labels are valid, including repeated labels inside one operand axis
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
8. If an operand contains several repeated-label groups, their normalization
   MUST be equivalent to applying Rule 7 simultaneously against original
   source-axis positions. An implementation MAY sequence the operations only
   if it remaps later axis positions and preserves that result.
9. Right-hand-side labels MUST be unique
   (`CONTRACT_OUTPUT_LABEL_DUPLICATE` otherwise) and MUST be drawn from the
   union of all input labels (`CONTRACT_OUTPUT_LABEL_UNBOUND` otherwise).
   Validation scans right-hand-side occurrences left-to-right, checking whether
   each label is bound before checking whether that occurrence is a duplicate.
10. Lowering MUST first diagonal-normalize every operand under Rules 7-8, then
    perform explicit label alignment, elementwise `MUL`, and
    `REDUCE_AXIS_SUM`.
11. Broadcast alignment order MUST be deterministic:
   1. `out_labels`: output labels in right-hand-side order.
   2. `red_labels`: unique labels not in outputs, in first-appearance order
      across left-hand-side operands.
   3. `global_order = out_labels ++ red_labels`.
   4. Each normalized operand is permuted to schema-label order, receives
      logical size-1 axes for labels absent from that operand, and is expanded
      to `global_order`.
   5. Schema labels, not positional trailing-rank promotion, select the source
      axis mapped to each position in `global_order`.
12. Implementations MAY replace the lowered pattern, including `DIAGONAL`, with
    backend-specific kernels (for example GEMM/WMMA), but observable semantics
    MUST remain equivalent. For fixed-width integer results, equivalence
    includes the canonical contributor traversal, checked promoted-dtype left
    fold, and first-failure behavior in `../stdlib/conventions.md`.
13. Empty axis lists are allowed on both LHS and RHS and denote rank-0 axes (for example `contract{ i, i -> }`, `contract{ i i -> }`, `contract{ , i -> i }`).
14. `contract` with empty RHS MUST return a rank-0 tensor value (not an implicit scalar).
15. Integer multiply or sum overflow in the fixed lowering MUST propagate as
    `Err(NumericError)` with cause `INT_OVERFLOW`. The sum stage MUST use the
    checked integer reduction baseline in `../stdlib/conventions.md`.
16. If `red_labels` has reduction volume `0`, each existing output coordinate
    MUST receive the additive identity in the promoted contract result dtype
    under the empty-domain contract in `../stdlib/conventions.md`.
17. Every fresh `contract{...}` schema starts in relaxed name-checking mode with
    an empty alias map. Name-mode and alias modifiers configure only the schema
    value they return; they do not establish ambient state or propagate to any
    other `contract{...}` expression.

`einop{...}`:

1. Every operand MUST have a concrete carrier satisfying `ReadableTensor`.
2. The parser accepts the shared transform `AxisExpr` grammar. The v0.1
   semantic subset for `einop` permits identifier and integer-literal leaves,
   `*`, and parentheses. Axis-pack tokens remain permitted as axis-list entries
   under the shared pack rules.
3. Any `+` node at any depth in a left-hand-side or right-hand-side `AxisExpr`
   MUST fail with `AXIS_MAPPING_UNSUPPORTED`. This schema-level check visits
   left-hand-side operand lists first and right-hand-side output lists second,
   in lexical source order within each side, and reports the earliest `+` token.
   It MUST complete before dimension-equation construction, dimension solving,
   structural mapping, dtype selection, or Core Tensor IR lowering.
4. An accepted atomic/product-only schema combines structural grouping with
   fixed multiply+sum elimination under the authoritative normalization and
   lowering algorithm below.
5. `reduce_by`/`contract_by` customization is undefined for `einop` in v0.1;
   `.by` and related accessor attempts MUST fail with `METHOD_NOT_FOUND`.

#### `einop` normalization and lowering

One `einop` schema is normalized once, before any output is evaluated. This
section is the authoritative parse-to-Core-Tensor-IR algorithm; catalog and
conformance documents MUST NOT define a competing lowering.

Validation and shared solving proceed as follows:

1. The implementation MUST apply the shared transform grammar, arity,
   axis-pack, `.with_sizes(...)`, and diagnostic-precedence rules before
   execution. An output-side anonymous pack has no input binding under the
   one-anonymous-occurrence rule and MUST fail with `AXIS_PACK_UNBOUND`.
2. Within one right-hand-side output, each identifier and named-pack token MUST
   occur at most once. A repeated occurrence MUST fail with
   `EINOP_OUTPUT_LABEL_DUPLICATE` before dimension solving, even when a repeated
   pack could bind zero axes. Separate outputs are checked independently in
   right-hand-side order.
3. Pack partitioning and dimension solving MUST run exactly once for the whole
   schema. Every operand and every output participates in that one solution.
   An identifier that occurs only on the right-hand side is a free dimension
   unless fixed by `.with_sizes(...)`; omission therefore follows the shared
   free-variable rule and fails with `AMBIGUOUS_DIMS`.
4. Reusing the same output-only identifier in different outputs reuses its one
   solved extent, but does not couple output evaluation or storage.

After the shared solution is known, each left-hand-side operand is expanded to
an ordered sequence of logical axis keys:

1. Product leaves are visited in lexical left-to-right order under the
   rightmost-factor-fastest coordinate mapping. A physical product axis is
   exposed as its logical factor axes with `RESHAPE` before later steps.
2. Every identifier `x` receives the schema-global key `label(x)`. Equal
   identifier spellings in any operand denote the same key.
3. Member `j` of a named pack `...p` receives the schema-global key
   `pack(p,j)`. Repeated occurrences of `...p` reuse those keys memberwise.
4. Every integer-literal occurrence receives a fresh input key identified by
   its operand and source occurrence. Equal literal values do not create
   identity.
5. Every member of an anonymous input pack receives a fresh input key. Those
   keys cannot be named by an output.
6. All occurrences of one schema-global key MUST have exactly the same extent;
   a conflict fails with `INCONSISTENT_DIMS` under the shared solver rules.
7. If one operand contains a schema-global key more than once, those logical
   axes MUST be normalized with `DIAGONAL`, simultaneously against their
   pre-diagonal positions, retaining the first occurrence. Repetition across
   different operands is alignment, not diagonal extraction. Fresh literal
   and anonymous-pack keys never participate in diagonalization.

Each right-hand-side output is then expanded to an ordered sequence of output
keys:

1. An identifier present in the left-hand-side identifier set selects its
   existing `label(x)` key.
2. An identifier absent from that set creates a new output key with the extent
   supplied by its required `.with_sizes(...)` binding. It never renames or
   matches an anonymous input axis, even when extents are equal.
3. Every integer-literal occurrence creates a fresh output key of that literal
   extent. It never matches an equal-valued input literal occurrence.
4. A named output pack expands to its already-bound `pack(p,j)` keys in member
   order. An unbound named output pack fails with `AXIS_PACK_UNBOUND` under the
   shared pack rules.
5. Product leaves determine logical key order; the written top-level axis-list
   entries retain the physical grouping required for the final output.

The result dtype of output `q` is normalized from the schema and static input
element dtypes only:

1. Let input dtypes in operand order be `T0, ..., T(n-1)`, where `n >= 1`.
   Define `P0 = T0` and `P(k+1) = Promote(Pk, T(k+1))`. Let `P` be the final
   source-order fold result.
2. Output `q` is *reduction-capable* when any left-hand-side identifier or named
   pack is omitted from that output, or when any left-hand-side literal or
   anonymous pack occurs. This classification is syntactic: an omitted pack
   makes the output reduction-capable even when its runtime binding is empty.
3. If `n == 1` and output `q` is not reduction-capable, `Uq = T0`. New output
   axes alone do not trigger promotion.
4. Otherwise, `Uq = Promote(P,P)` when output `q` is reduction-capable, and
   `Uq = P` when it is not. The extra self-promotion makes a unary `bool`
   reduction use `u8` while a unary structural `bool` output remains `bool`.
5. Runtime dimensions, pack lengths, empty domains, and backend choices MUST
   NOT change `Uq`. A multi-output schema MAY therefore have different static
   element dtypes for different outputs.

Every output is an independent multiply+sum operation over all original
operands and lowers in these steps:

1. `out_keys` is the flattened output-key sequence in written order.
2. `red_keys` is every unique normalized input key absent from `out_keys`, in
   first-appearance order over operands and then logical source axes.
3. `global_order = out_keys ++ red_keys`.
4. Each diagonal-normalized operand is permuted into its relative
   `global_order`, receives logical size-one axes for every absent key, and is
   expanded to the solved extent of every key. New output keys are therefore
   explicit directed expansion axes, not implicit renames.
5. Every operand element is converted directly to `Uq` under the numeric
   promotion conversion rules before multiplication. Implementations MUST NOT
   multiply an early operand prefix in a narrower intermediate dtype.
6. The converted operands are multiplied with checked `MUL` in original
   operand order. With one operand there is no `MUL`; its converted value is
   the contribution.
7. If `red_keys` is non-empty, one `REDUCE_AXIS_SUM` consumes those axes in
   their listed order. It MUST implement the canonical contributor traversal
   and checked left fold from `../stdlib/conventions.md`; sequencing separate
   reductions or reassociating contributors is invalid unless observably
   equivalent. With no `red_keys`, this step is absent.
8. The remaining flat `out_keys` are grouped into the written output axis-list
   entries with `RESHAPE`. Product factors use the shared
   rightmost-factor-fastest order; named-pack members remain separate physical
   axes; an empty output axis list produces rank 0.
9. `REALIZE` materializes one owning `Tensor[Uq]`.

The required Core Tensor IR path is therefore:

```text
RESHAPE
-> DIAGONAL
-> PERMUTE / RESHAPE(size-one insertion) / EXPAND
-> CAST(kind=numeric)*
-> MUL*
-> REDUCE_AXIS_SUM(ordered red_keys)?
-> RESHAPE
-> REALIZE
```

Output and failure behavior is fixed:

1. Output coordinates are visited in logical row-major order over `out_keys`;
   contributor coordinates use row-major order over `red_keys`. A zero extent
   in `out_keys` produces no output coordinates and performs no element
   arithmetic for that output.
2. A non-empty reduction slice initializes its `Uq` accumulator from the first
   successfully multiplied contributor. A reduction volume of zero produces
   the additive identity in `Uq` for every existing output coordinate.
3. Integer promotion conversion, multiplication, or sum overflow propagates as
   `Err(NumericError)` with cause `INT_OVERFLOW` and operation `einop`.
4. Outputs are evaluated in right-hand-side order and returned in that order.
   A single output returns `Tensor[U0]`; `N >= 2` outputs return
   `(Tensor[U0], ..., Tensor[U(N-1)])`.
5. A multi-output invocation is expression-atomic. The first output failure in
   right-hand-side evaluation order returns `Err` and exposes no partial tuple.
6. Common-subexpression sharing, contraction fusion, and backend-specific
   kernels are optimizations only. They MUST preserve output values, static
   dtypes, materialization, checked-arithmetic order, and first-failure
   behavior defined above.


### Reducer Determinism and Parallelism

For `reduce{...}.reduce_by(f)`, implementations MUST call `f` once per output coordinate with the full reduced subtensor view for that coordinate.

Implementations MAY parallelize across output coordinates.

Implementations MUST NOT split and reassociate one reduced subtensor internally unless an explicit algebraic contract for the reducer is declared by a future extension.

v0.1 does not assume reducer identity/associativity/commutativity for user-supplied `f` in `reduce{...}.reduce_by(f)`.

The fixed traversal and checked left fold for standard integer sum reductions
are defined separately in `../stdlib/conventions.md`; they do not grant
algebraic laws to user-supplied reducers.

Empty reduced-slice behavior is reducer-defined in v0.1:

1. empty reduced subtensors are still passed to `f`,
2. output coordinate result follows `f` return (`Ok` value or propagated `Err`).

For ordered phase reducer form:

1. phases execute in source order,
2. each phase axis set MUST only reference reduced axes (`REDUCE_BY_PHASE_NON_REDUCED_AXIS` on violation),
3. phase axis sets MUST be disjoint (`REDUCE_BY_PHASE_OVERLAP` on violation),
4. phase axis set union MUST equal the full reduced-axis set (partial coverage forbidden; `REDUCE_BY_PHASE_INCOMPLETE` on violation),
5. axis-partition validation precedes callback-chain typing,
6. phase `i` receives one `TensorView[T_i]` for each coordinate left after
   removing its phase axes and its `Ok(T_(i+1))` values form the next logical
   intermediate tensor,
7. an `Err(E_i)` aborts the invocation before any later phase executes and
   exposes no partial output,
8. implementations MAY parallelize coordinates within a phase but MUST preserve
   the phase-success barrier.

Unordered dictionary/map reducer plans MUST NOT define reduction order and are disallowed.

For floating-point reduction operators:

1. Default and strict determinism profile semantics are defined by `../stdlib/conventions.md`.
2. APIs expose profile selection only where their standard-library signature includes a typed `profile: DeterminismProfile` parameter.
3. Any strict deterministic profile request MUST preserve a fixed, documented reduction order for identical inputs.


### Numeric Runtime Edge Rules

Numeric runtime edge behavior is shared across primitive tensor arithmetic and all lowered higher-level tensor ops:

1. Fixed-width integer overflow is checked and MUST fail with `INT_OVERFLOW`.
2. Integer `div` and `rem` MUST use the scalar-shared truncating
   quotient/remainder law in `../stdlib/conventions.md`.
3. Integer `div` by zero MUST fail with `INT_DIV_BY_ZERO`; integer `rem` by
   zero MUST fail with `INT_REM_BY_ZERO`.
4. Signed `div(MIN, -1)` MUST fail with `INT_OVERFLOW`, while
   `rem(MIN, -1)` MUST succeed with `0`.
5. Floating arithmetic is IEEE-754 value semantics:
   1. divide-by-zero and overflow produce Inf/NaN as applicable;
   2. domain-invalid operations produce NaN;
   3. signed-zero behavior is preserved (`1.0 / -0.0` is `-Inf`, `1.0 / +0.0` is `+Inf`).
6. Explicit wrapping arithmetic APIs are required for intentional modular arithmetic and MUST use two's-complement modulo semantics.
7. Numeric cast edge semantics are normative in v0.1:
   1. float-to-int uses truncation toward zero when input is finite and in destination range;
   2. float-to-int with NaN/Inf/out-of-range MUST fail with `CAST_INVALID_VALUE`;
   3. integer narrowing out-of-range MUST fail with `CAST_INVALID_VALUE`;
   4. `wrapping_cast` MUST be integer-domain-only and uses modulo `2^bits(dst)` semantics;
   5. `bitcast` MUST preserve bit-pattern exactly and requires equal source/destination bit width.
8. Invalid dtype-pair use of `wrapping_cast` or `bitcast` MUST fail with `CAST_INVALID_DTYPE_PAIR`.
9. Binary arithmetic and contraction MUST follow the scalar/tensor-shared numeric promotion lattice in `../stdlib/conventions.md`.
10. Tensor `exp`, `log`, `sqrt`, and `tanh` accept only floating and complex
    element dtypes and preserve the input element dtype. `Bool` and integer
    inputs return `Err(DTypeError)` without implicit promotion.
11. Unary real-domain primitives MUST NOT implicitly promote real inputs to complex outputs in v0.1.
12. Unary transcendental NaN, infinity, signed-zero, real-domain, and complex
    principal-branch behavior MUST follow the exact contract in
    `../stdlib/tensor.md`.
13. Error-vs-value outcomes for this section MUST remain profile-invariant between `default` and `strict`.
14. Expression operator `/.` MUST use fractional-division carrier typing (`FDivCarrier`) and IEEE floating/complex divide semantics from `../stdlib/conventions.md`.

Detailed API-level contracts are in `../stdlib/tensor.md` and `../stdlib/conventions.md`.


### `map_axis` as First-class Transform

`map_axis` is a pure transform and MUST NOT be bang.

The canonical API is defined in `../STDLIB.md`.

Normative behavior:

1. `map_axis` maps over one selected axis per mapped input and stacks successful callback results at `out_axis`.
2. Each mapped callback parameter preserves its concrete carrier
   position-wise. A `Tensor[T]` argument produces owning `Tensor[T]` steps; a
   `TensorView[T]` argument produces zero-copy retained `TensorView[T]` steps.
   One callback MAY mix the two carriers across positions.
3. Unmapped inputs (`in_axes=None`) are passed unchanged at each mapped step.
4. Callback results MAY use either concrete readable carrier, directly or under
   the accepted `Res`/`Opt` wrappers, but successful stacking always produces a
   materialized `Tensor`.
5. `map_axis` MUST NOT insert a carrier conversion or `materialize()` call at
   an input or callback boundary. `TensorView` results are read directly into
   the final stacked tensor.
6. Observational semantics are equivalent to left-to-right evaluation over batch index `i = 0..B-1`.
7. Implementations MAY execute in parallel but MUST preserve deterministic output and failure index equivalence to the left-to-right model.
8. `map_axis` MUST reject effectful callbacks/functions under the same purity split rule as other non-bang combinators (`PURE_COMBINATOR_EFFECTFUL_CALLBACK`).
   This core-owned callback-effect validation precedes tensor-specific
   `map_axis` argument validation for the same construction.
9. If mapped function evaluation fails for `Res`/`Opt` forms, the earliest failing 0-based index MUST be reported.
10. `map_axis` is an observational map-and-stack operation. It does not imply primitive batching, fusion, or vectorized lowering.

### Core Tensor IR vs Kernel IR

`grad` and similar program transforms are defined on a pure graph layer, not on kernel code generation internals.

Normative boundary:

1. A Core Tensor IR graph is a pure tensor graph composed of primitive tensor operations (elementwise arithmetic, movement/view transforms, reductions, gather/scatter, realization barriers).
2. Kernel IR (uop/codegen layer) includes low-level execution operations (for example `LOAD`, `STORE`, `INDEX`, `RANGE`, `IF`, `BARRIER`) and effectful scheduling/runtime behavior.
3. `grad` MUST be defined only for programs lowerable to Core Tensor IR.
4. Programs requiring Kernel IR-only constructs in the differentiable path MUST fail with gradient diagnostics (`GRAD_LOWERING_FAILED` or `GRAD_UNSUPPORTED_PRIMITIVE`).
5. Core Tensor IR is downstream of the bounded tensor source layer. It MUST
   NOT be constructed for a non-tensor compilation or before applicable tensor
   source and constraint validation succeeds.


### v0.1 Canonical AD-Support Graph

For one `grad` or `value_and_grad` invocation, the selected `argnum` defines the
only differentiated argument. Support is classified on a canonical
AD-support graph, not on a backend-specific executable decomposition.

The graph and its operand edges are classified as follows:

1. The canonical AD-support graph is the backward def-use closure of the
   successful scalar primal result after source validation and Core Tensor IR
   lowering, annotated with activity provenance from the selected argument.
   Nodes with no def-use path to that result are not part of the support graph.
   `Res`/`Opt` short-circuit behavior remains outside this successful graph.
2. Value activity is a def-use provenance property. The selected formal
   parameter is active, and a Core result is active when any operand value in
   its defining node has active provenance. This classification occurs before
   algebraic simplification, constant folding, or common-subexpression
   replacement that could erase provenance. For example, both inputs and the
   result of `SUB(x, x)` remain active even though the mathematical result is
   zero.
3. Operand role is independent of value activity. Every operand slot is either
   a tensor-data slot or an auxiliary slot. Tensor operands denoting logical
   element values use data slots. Index values, axis lists, shapes, dtype
   descriptors, and other non-cotangent metadata use auxiliary slots.
4. A *source-active edge* supplies an active value to a tensor-data slot. The
   closed table below identifies the data slots that carry cotangents in v0.1;
   an active data slot omitted from that table is unsupported. A data edge
   carrying a value without active provenance is passive. An auxiliary slot in
   the v0.1 required set MUST also receive a value without active provenance.
   Static tensor-shape metadata satisfies this requirement; an index tensor
   derived from differentiated tensor values does not.
5. A passive subgraph MAY contain any otherwise-valid Core Tensor IR primitive
   because no cotangent is propagated through that subgraph.
6. After the `grad` API's type and result-family validation, an implementation
   MUST accept a callback exactly when it can form the canonical graph and every
   source-active edge is admitted by the closed table below. An active edge in
   any omitted primitive or operand slot, or an active value supplied to an
   auxiliary slot, MUST fail with `GRAD_UNSUPPORTED_PRIMITIVE`.
7. A callback whose scalar result has no def-use provenance from the selected
   argument has an empty source-active graph and is supported. Its returned
   gradient MUST be an all-zero owning tensor with the selected argument's
   logical shape and dtype.

The v0.1 source-active table is closed:

| Core primitive | Admitted source-active data slots | Required conditions |
|---|---|---|
| `MUL` | either or both data operands | Active operands and the result use floating element dtypes. Any conversion remains a separate `CAST` node. |
| `MAX` | either or both data operands | Active operands and the result use floating element dtypes; the v0.1 uniform tie policy applies. |
| `REDUCE_AXIS_SUM` | data operand | Input and result use floating element dtypes; `ordered_axes` is auxiliary. |
| `REDUCE_AXIS_MAX` | data operand | Input and result use floating element dtypes; `ordered_axes` is auxiliary and the v0.1 uniform tie policy applies. |
| `RESHAPE` | data operand | The target shape is auxiliary shape metadata. |
| `EXPAND` | data operand | The target shape is auxiliary shape metadata. |
| `GATHER` | source-data operand | Every index operand is auxiliary and passive with respect to the selected argument. Duplicate passive indices are supported. |
| `REALIZE` | data operand | Materialization preserves logical values. |

The support classifier MUST preserve the semantic primitive identities chosen
by existing operation-family lowering rules:

1. An operation family's normative logical Core lowering takes precedence over
   the AD support fallback rules below. Required `DIAGONAL`, `PERMUTE`, or
   non-identity `CAST` nodes are therefore visible to the closed support table
   and cannot be replaced merely to gain AD acceptance.
2. Explicit primitive calls retain their Core primitive identity.
3. An implicit promotion whose canonical source and destination dtypes are
   equal emits no `CAST`; the `CAST(kind=numeric)*` stage in a required lowering
   contains only non-identity conversions. An explicit source `astype`,
   `wrapping_cast`, or `bitcast` retains its `CAST` identity even when its source
   and destination dtypes are equal.
4. Only when an operation family specifies a logical coordinate map without a
   more specific Core identity does the classifier use these fallbacks: a
   row-major-order-preserving regrouping uses `RESHAPE`, a fixed
   output-to-source coordinate map uses `GATHER`, explicit duplication uses
   `EXPAND`, and materialization ends in `REALIZE`.
5. Surrounding `RESHAPE` nodes MAY expose or regroup logical axes for Rule 4.
6. A later backend decomposition, fusion, view optimization, or replacement of
   one logical primitive by equivalent kernel operations MUST NOT change the
   support decision or its diagnostic.

Reverse execution has a separate closed requirement. An implementation MUST be
able to execute the reverse graph required by every accepted source-active
graph using:

```text
ADD, MUL, FDIV, CMPLT, CMPEQ, WHERE, ISNAN,
CAST(kind=numeric), RESHAPE, EXPAND, REDUCE_AXIS_SUM,
SCATTER_ADD, REALIZE
```

It MUST also be able to construct the scalar cotangent seed and zero-valued
cotangents of a required shape and floating dtype. Those constructors are
compiler-internal AD obligations, not additional source-callable Core
primitives.

This reverse-execution set specifies required execution capability only. It
does not prescribe per-primitive VJP equations, reverse broadcast axes or
ordering, saved primal values, recomputation, or checkpointing.

The reverse-execution set does not enlarge the source-active table. In
particular, source-active `ADD`, `FDIV`, comparisons, `WHERE`, `ISNAN`, any
`CAST`, and every `SCATTER_*` operation remain unsupported in v0.1 even though
the generated reverse graph can use some of them. A callback produced by
`grad` or `value_and_grad` is not itself an accepted differentiation input in
v0.1; attempting higher-order differentiation MUST fail with
`GRAD_UNSUPPORTED_PRIMITIVE`.

Failure classification is ordered:

1. If the callback cannot form a pure Core Tensor IR graph, including because
   it requires runtime tensor-value-dependent control flow, recursion, or a
   Kernel IR-only construct, it MUST fail with `GRAD_LOWERING_FAILED`.
2. If Core lowering succeeds but the canonical AD-support graph violates the
   source-active table or passive-auxiliary requirement, it MUST fail with
   `GRAD_UNSUPPORTED_PRIMITIVE`.
3. Once a graph is accepted, inability of a backend to execute its required
   reverse graph is an implementation nonconformance; it MUST NOT be reported
   to the program as either support diagnostic.


### `grad` as First-class Transform

`grad` is a pure transform and MUST NOT be bang.

The canonical APIs are defined in `../STDLIB.md`.

Normative behavior:

1. `grad(f)` and `value_and_grad(f)` MUST return callables whose invocation result is wrapped in `Res[...]`.
2. The differentiated argument (`argnum`) MUST refer to a concrete `Tensor[T]`
   or `TensorView[T]` argument with floating dtype. The returned callable
   preserves that exact parameter carrier and every other parameter type.
3. v0.1 scalar-gradient mode requires primal output to be scalar (`rank == 0`), unless an explicit future Jacobian API is used.
4. `grad` evaluation MUST:
   1. Lower `f` to Core Tensor IR.
   2. Run reverse-mode AD on that primitive graph.
   3. Return an owning gradient `Tensor[T]` matching the differentiated
      argument's logical shape.
5. If `f` returns `Res[...]`, `Err(e)` MUST short-circuit without gradient computation.
6. If `f` returns `Opt[...]`, `None` MUST short-circuit without gradient computation.
7. Gradient result dtype MUST equal primal differentiated argument dtype.
8. Implementations MAY use higher precision internally, but final returned gradient MUST be cast to primal dtype.
9. For a differentiated `TensorView[T]`, the gradient is with respect to the
   supplied view's logical values and shape. `grad` MUST NOT implicitly return
   a source/base-shaped cotangent.
10. AD-lowerability is a property of the complete Core Tensor IR path, not a
    capability implied by either concrete tensor carrier.
11. `MAX`/`REDUCE_AXIS_MAX` VJP policy in v0.1 MUST be:
   1. NaN-propagating in forward semantics.
   2. Tie gradients distributed uniformly across all maximal contributors.
   3. If NaN contributors exist in a reduced slice, treat NaN contributors as the tie set for subgradient distribution.
12. Source-observable acceptance and rejection MUST follow the closed
    [v0.1 canonical AD-support graph](#v01-canonical-ad-support-graph).


### Gather/Scatter Determinism Baseline

Gather/scatter semantics are defined at tensor graph level and MUST satisfy the
repeatability contract in both profiles and the canonical baseline-bit contract
in strict profiles.

1. `GATHER` is pure and index-ordered by the logical index tensor traversal order.
2. `SCATTER(SET)` with duplicate indices MUST fail with `SCATTER_SET_DUPLICATE_INDEX`.
3. `SCATTER(ADD/MAX/MIN)` updates use a fixed baseline order equal to increasing
   logical update traversal (`i = 0..n-1`). In that baseline evaluation,
   repeated targets observe the candidate value produced by every earlier
   update to that target.
4. Fixed-width integer `SCATTER(ADD)` begins from the destination value and
   performs a checked destination-dtype addition and store for each update.
   Under both profiles, implementations MUST NOT use a wider accumulator, defer
   the range check until the final store, or reassociate updates across an
   intermediate `INT_OVERFLOW`.
5. Profile-specific scatter behavior is:
   1. default profile: a compiled artifact or execution plan MAY select a
      parallel reassociation for floating-point accumulation, but repeated
      evaluations under that fixed execution identity MUST reproduce the same
      result bits;
   2. strict profile: bitwise-equivalent results to baseline order are required.
6. The v0.1 scatter request surfaces are `std::tensor::index::set_at(..., profile=DeterminismProfile::Strict)` and `std::tensor::index::set_at!(..., profile=DeterminismProfile::Strict)` as specified in `../stdlib/tensor.md`.

This section uses the primitive-level determinism policy from `../stdlib/conventions.md`:

1. policy applies to all Core Tensor IR primitives;
2. high-level tensor operations inherit by lowering;
3. an unavailable explicit strict request through a profile-bearing API MUST
   fail with `DETERMINISM_UNAVAILABLE`; profile-less active strict context
   establishment follows `../stdlib/conventions.md`.
4. Physical primitive execution schedule is implementation-defined unless an
   operation rule fixes logical order for observable checked behavior. Every
   physical schedule MUST preserve the resulting normative values, diagnostics,
   and first-failure boundary. A selected schedule capable of changing floating
   result bits is part of the execution plan identity; it MUST NOT vary between
   repeated evaluations under that identity.


### Failure Selection and Visibility

Tensor-expression failure behavior follows `../stdlib/conventions.md`:

1. validation vs execution failure precedence is fixed by stage ordering;
2. runtime execution failures are fail-fast at first failing logical index under deterministic traversal semantics;
3. failed expressions do not yield values, and partial intermediate tensor state MUST NOT be exposed as successful results.


### Excluded `Tensor[Tri]` Extension

`Tensor[Tri]` support is not defined in v0.1.
`Tri` is outside the v0.1 cast-matrix axes; neither the scalar logic type nor
its explicit-conversion follow-up implies tensor `DType` or tensor `astype`
support.

If implementations provide `Tensor[Tri]` experimentally, they MUST document:

1. allowed operation families (logic-only vs mixed arithmetic);
2. reduction semantics for three-valued logic;
3. interaction with control flow extraction to scalar `Tri`/`Bool`.

`Tensor[Tri]` behavior is excluded from required v0.1 conformance suites.
