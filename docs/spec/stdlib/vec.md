# `std::vec` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Type

`Vec[T]` is a resizable random-access array.

```eglur-catalog
opaque type Vec[T]
```

`IndexError` in this module is the shared `std::prelude::IndexError` type.

Literal note:

1. `[...]` does not construct `Vec[T]` in v0.1; it constructs `Seq[T]`.
2. `Vec[T]` values are obtained via `Vec[T].new()` and/or explicit conversion from `Seq[T]` (`to_vec()`).
3. This is an intentional v0.1 baseline boundary, not an implementation gap:
   1. array literals normalize first to sequence semantics,
   2. mutable/random-access capability is acquired explicitly.

`Vec` supports both:

1. Pure value updates (non-bang) returning new values.
2. In-place updates (bang) mutating `&mut Self`.

### Equality

`Vec[T]` implements `Eq` iff `T` implements `Eq`.

1. Two vectors are equal iff they have the same length and each pair of elements at the same position is equal under `Eq.eq`.
2. Element pairs MUST be compared from left to right and comparison MUST short-circuit at the first unequal pair.
3. Equality between different `Vec` instantiations MUST fail with `COMPARISON_TYPE_MISMATCH`; implementations MUST NOT insert implicit element casts.

### Hashing

`Vec[T]` implements `Hash` iff `T` implements `Hash`.

1. Hashing starts with the caller-supplied seed and hashes the vector length as a `Nat`.
2. Elements are then hashed in increasing index order, passing each result as the next element's seed.
3. Hashing MUST NOT mutate the vector or expose its storage representation.

### Constructors and Queries (pure)

```eglur-catalog
fn Vec[T].new() -> Vec[T]
optional fn Vec[T].with_capacity(cap: Nat) -> Vec[T]

fn Vec[T].len(self: &Self) -> Nat
fn Vec[T].is_empty(self: &Self) -> Bool
fn Vec[T].get(self: &Self, i: Nat) -> Opt[T]
fn Vec[T].first(self: &Self) -> Opt[T]
fn Vec[T].last(self: &Self) -> Opt[T]
fn Vec[T].contains(self: &Self, x: &T) -> Bool requires Eq[T]
fn Vec[T].index_of(self: &Self, x: &T) -> Opt[Nat] requires Eq[T]
fn Vec[T].slice(self: &Self, start: Nat, end: Nat) -> Res[Vec[T], IndexError]
```

Size and index boundary rules:

1. Lengths, capacities, and indices in `std::vec` are `Nat` values.
2. Integer literals in these argument positions are contextually typed as `Nat`.
3. Signed runtime values MUST be converted with `Nat.from_i64` before use; failed conversion uses `DOMAIN_INVALID_VALUE`.
4. `IndexError` covers bounds/range failures after a valid `Nat` value has crossed the API boundary.

Bounds and query semantics:

Let `len` be the receiver length.

1. `get(i)` returns `Some(value)` when `i < len`; otherwise it returns `None`.
2. `first()` is equivalent to `get(0)` except that it returns `None` for an empty vector without requiring a caller-provided index.
3. `last()` returns `None` when `len == 0`; otherwise it returns the final element.
4. `slice(start, end)` uses end-exclusive bounds:
   1. it returns `Ok` with elements at positions `start, start+1, ..., end-1` when `start <= end <= len`,
   2. `start == end` returns an empty `Vec`,
   3. `start > end` or `end > len` returns `Err(IndexError)`.
5. `index_of(x)` returns the first zero-based `Nat` position whose element is equal to `x`, or `None` when no element matches.


### Pure Value Updates (non-bang)

```eglur-catalog
fn Vec[T].push(self: Vec[T], x: T) -> Vec[T]
fn Vec[T].extend(self: Vec[T], xs: Seq[T]) -> Vec[T]
fn Vec[T].insert(self: Vec[T], i: Nat, x: T) -> Res[Vec[T], IndexError]
fn Vec[T].remove(self: Vec[T], i: Nat) -> Res[(Vec[T], T), IndexError]
fn Vec[T].concat(self: Vec[T], other: Vec[T]) -> Vec[T]
fn Vec[T].reverse(self: Vec[T]) -> Vec[T]
fn Vec[T].dedup(self: Vec[T]) -> Vec[T] requires Eq[T]
fn Vec[T].retain(self: Vec[T], p: callback[pure](T) -> Bool) -> Vec[T]
fn Vec[T].map[U](self: Vec[T], f: callback[pure](T) -> U) -> Vec[U]
fn Vec[T].filter(self: Vec[T], p: callback[pure](T) -> Bool) -> Vec[T]

fn Vec[T].sort(self: Vec[T]) -> Vec[T] requires Ord[T]
fn Vec[T].sort_by[K](self: Vec[T], key: callback[pure](T) -> K) -> Vec[T] requires Ord[K]
```

Value-update semantics:

1. These methods are pure non-bang operations and MUST NOT mutate the caller-visible source binding.
2. The returned `Vec` MUST be observationally independent from the source value: later bang mutation of one value MUST NOT change the other.
3. Implementations MAY copy, move, or share storage internally, but such representation choices MUST NOT be observable.
4. `T` is ownership-free and reference-free in v0.1; ownership-bearing or reference-bearing vector elements are outside the baseline value-only generic domain.
5. `insert(i, x)` returns `Ok(new_vec)` when `i <= len`; insertion at `i == len` appends `x`.
6. `insert(i, x)` returns `Err(IndexError)` when `i > len`; the source value remains observationally unchanged.
7. `remove(i)` returns `Ok((new_vec, removed))` when `i < len`.
8. `remove(i)` returns `Err(IndexError)` when `i >= len`; the source value remains observationally unchanged.

Deduplication semantics:

1. `dedup` and `dedup!` MUST remove every element whose earlier equal element has already been kept.
2. Equality is determined by `Eq[T]`.
3. The first occurrence of each equal-value class MUST be kept.
4. Encounter order among kept elements MUST be preserved.
5. Deduplication is not adjacent-only and MUST NOT sort elements.

Sorting semantics:

1. `sort` and `sort_by` MUST be stable.
2. For elements with equal sort key, output order MUST preserve original encounter order.
3. Every element or key comparison MUST dispatch the canonical `std::prelude::Ord.cmp` implementation; sorting MUST NOT use a separate intrinsic or operator-specific ordering.
4. For floating sort keys (`f16`, `bf16`, `f32`, `f64`), comparison MUST follow unified floating total-order semantics from `conventions.md`.
5. For floating sort keys, NaN keys MUST appear after all non-NaN keys.
6. For floating sort keys, signed zero MUST remain ordered as `-0.0 < +0.0`.
7. `sort_by` and `sort_by!` key callbacks are pure even though `sort_by!` mutates its receiver; an effectful key callback MUST fail with `PURE_COMBINATOR_EFFECTFUL_CALLBACK`.


### In-place Updates (bang)

```eglur-catalog
fn Vec[T].push!(self: &mut Self, x: T) -> Unit
fn Vec[T].pop!(self: &mut Self) -> Opt[T]
fn Vec[T].set!(self: &mut Self, i: Nat, x: T) -> Res[Unit, IndexError]
fn Vec[T].insert!(self: &mut Self, i: Nat, x: T) -> Res[Unit, IndexError]
fn Vec[T].remove!(self: &mut Self, i: Nat) -> Res[T, IndexError]
fn Vec[T].clear!(self: &mut Self) -> Unit
fn Vec[T].truncate!(self: &mut Self, n: Nat) -> Res[Unit, IndexError]
fn Vec[T].reverse!(self: &mut Self) -> Unit
fn Vec[T].dedup!(self: &mut Self) -> Unit requires Eq[T]
fn Vec[T].retain!(self: &mut Self, p: callback[effectful](T) -> Bool) -> Unit
fn Vec[T].reserve!(self: &mut Self, additional: Nat) -> Unit
fn Vec[T].resize!(self: &mut Self, n: Nat, fill: T) -> Unit requires Clone[T] when n > self.len()
fn Vec[T].sort!(self: &mut Self) -> Unit requires Ord[T]
fn Vec[T].sort_by![K](self: &mut Self, key: callback[pure](T) -> K) -> Unit requires Ord[K]
```

In-place update semantics:

Let `len` be the receiver length before the operation begins.

1. `set!(i, x)` succeeds only when `i < len`; otherwise it returns `Err(IndexError)`.
2. `insert!(i, x)` succeeds only when `i <= len`; insertion at `i == len` appends `x`, and `i > len` returns `Err(IndexError)`.
3. `remove!(i)` succeeds only when `i < len`; otherwise it returns `Err(IndexError)`.
4. `truncate!(n)` succeeds only when `n <= len`; it removes the suffix beginning at `n`. If `n > len`, it returns `Err(IndexError)` rather than silently extending or treating the operation as a no-op.
5. Any `Err(IndexError)` result from `set!`, `insert!`, `remove!`, or `truncate!` MUST leave the receiver observationally unchanged.
6. `reserve!(additional)` is a capacity hint with a `Nat` count and MUST NOT change `len` or any element value. v0.1 does not define an allocation-failure value channel for this method.
7. `resize!(n, fill)` changes the logical length to exactly `n`:
   1. when `n < len`, it removes the suffix beginning at `n`,
   2. when `n == len`, it is a no-op,
   3. when `n > len`, it appends additional elements produced by `Clone.clone(&fill)`. If `T` does not implement `Clone`, calls that may extend the vector are rejected by ordinary type checking rather than by an `IndexError` path.
8. `resize!` is not an index/range operation and MUST NOT return `IndexError`; v0.1 does not define an allocation-failure value channel for this method.
9. `retain!` MUST invoke `p` exactly once for each element present when the call begins, in original index order; callback effects therefore occur left-to-right, and retained elements preserve their original relative order.

In-place sorting semantics:

1. `sort!` and `sort_by!` MUST be stable.
2. For elements with equal sort key, post-sort order MUST preserve original encounter order.
3. Every element or key comparison MUST dispatch the canonical `std::prelude::Ord.cmp` implementation; sorting MUST NOT use a separate intrinsic or operator-specific ordering.
4. For floating sort keys (`f16`, `bf16`, `f32`, `f64`), comparison MUST follow unified floating total-order semantics from `conventions.md`.
5. For floating sort keys, NaN keys MUST appear after all non-NaN keys.
6. For floating sort keys, signed zero MUST remain ordered as `-0.0 < +0.0`.


### Conversions (pure)

```eglur-catalog
fn Vec[T].freeze(self: Vec[T]) -> Seq[T]
fn Seq[T].to_vec(self: Seq[T]) -> Vec[T]
```
