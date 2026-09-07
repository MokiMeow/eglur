# `std::seq` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Concept

`Seq[T]` is a finite ordered sequence with deterministic iteration order.

An implementation MAY represent `Seq` lazily, but pure semantics MUST be equivalent to left-to-right evaluation when realized.

Effectful callback timing applies to every sequential `Seq` callable in this module whose callback type is `callback[effectful]`:

1. Each callback invocation MUST begin and return during the `Seq` callable invocation; the callable MUST NOT return with a callback invocation itself pending. An independently spawned `Task` retains its own execution timing.
2. The callback MUST run exactly once for each visited input element. A specified short-circuit or fail-fast boundary defines the visited prefix, and no callback may run after that boundary.
3. Realizing, iterating, copying, rendering, or converting a returned `Seq` MUST NOT invoke the callback or replay its effects.
4. A lazy internal representation is permitted only when it preserves this timing and replay contract.

```eglur-catalog
opaque type Seq[T]
```

Array literal binding:

1. v0.1 `ArrayExpr` surface (`[...]`) binds to `Seq[T]` by default.
2. Empty array literal `[]` requires expected type context; unresolved cases fail under `BINDING_TYPE_UNDETERMINED`.
3. `Vec[T]` usage from array literals requires explicit conversion (`to_vec()`).
4. This sequence-first literal policy is an intentional v0.1 baseline choice.

### Equality

`Seq[T]` implements `Eq` iff `T` implements `Eq`.

1. Two sequences are equal iff they have the same length and each pair of elements at the same position is equal under `Eq.eq`.
2. Element pairs MUST be compared from left to right and comparison MUST short-circuit at the first unequal pair.
3. Equality MAY realize a lazy sequence, but it MUST obey the callback timing and no-effect-replay contract in `Concept`.
4. Equality between different `Seq` instantiations MUST fail with `COMPARISON_TYPE_MISMATCH`; implementations MUST NOT insert implicit element casts.

### Hashing

`Seq[T]` implements `Hash` iff `T` implements `Hash`.

1. Hashing starts with the caller-supplied seed and hashes the sequence length as a `Nat`.
2. Elements are then hashed in encounter order, passing each result as the next element's seed.
3. Hashing MAY realize a lazy sequence, but it MUST obey the callback timing and no-effect-replay contract in `Concept`.

### Basics (pure)

```eglur-catalog
fn Seq[T].len(self: &Self) -> Nat
fn Seq[T].is_empty(self: &Self) -> Bool
```

`Seq[T].to_vec` is declared once by the [`std::vec` conversion surface](vec.md#conversions-pure).


### Pure Pipeline Combinators (REQUIRED)

```eglur-catalog
fn Seq[T].map[U](self: Seq[T], f: callback[pure](T) -> U) -> Seq[U]
fn Seq[T].filter(self: Seq[T], p: callback[pure](T) -> Bool) -> Seq[T]
fn Seq[T].flat_map[U](self: Seq[T], f: callback[pure](T) -> Seq[U]) -> Seq[U]
fn Seq[T].filter_map[U](self: Seq[T], f: callback[pure](T) -> Opt[U]) -> Seq[U]
fn Seq[Seq[T]].flatten(self: Seq[Seq[T]]) -> Seq[T]

fn Seq[T].take(self: Seq[T], n: Nat) -> Seq[T]
fn Seq[T].drop(self: Seq[T], n: Nat) -> Seq[T]
fn Seq[T].drop_while(self: Seq[T], p: callback[pure](T) -> Bool) -> Seq[T]

fn Seq[T].zip[U](self: Seq[T], u: Seq[U]) -> Seq[(T,U)]
fn Seq[T].enumerate(self: Seq[T]) -> Seq[(Nat,T)]
fn Seq[T].chunk(self: Seq[T], size: Nat) -> Seq[Seq[T]]
fn Seq[T].window(self: Seq[T], size: Nat) -> Seq[Seq[T]]

fn Seq[T].find(self: Seq[T], p: callback[pure](T) -> Bool) -> Opt[T]
fn Seq[T].find_map[U](self: Seq[T], f: callback[pure](T) -> Opt[U]) -> Opt[U]
fn Seq[T].any(self: Seq[T], p: callback[pure](T) -> Bool) -> Bool
fn Seq[T].all(self: Seq[T], p: callback[pure](T) -> Bool) -> Bool
fn Seq[T].partition(self: Seq[T], p: callback[pure](T) -> Bool) -> (Seq[T], Seq[T])
fn Seq[T].group_by[K](self: Seq[T], key: callback[pure](T) -> K) -> Map[K, Seq[T]] requires Eq[K] + Hash[K]

fn Seq[T].tap(self: Seq[T], f: callback[pure](&T) -> Unit) -> Seq[T]
```

Semantics:

1. `tap` callback `f` MUST be pure.
2. `tap` MUST preserve order, cardinality, and values.
3. Every callback-taking operation in this pure pipeline family (`map`, `filter`, `flat_map`, `filter_map`, `drop_while`, `find`, `find_map`, `any`, `all`, `partition`, `group_by`, and `tap`) MUST reject effectful callbacks with `PURE_COMBINATOR_EFFECTFUL_CALLBACK`.
4. `filter_map` and `find_map` MUST preserve left-to-right callback evaluation.
5. `find`/`find_map`/`any`/`all` MUST short-circuit:
   1. `find`/`find_map` stop at first matching element,
   2. `any` stops at first `true`,
   3. `all` stops at first `false`.
6. `partition` MUST preserve encounter order within both output partitions.
7. `flatten` MUST concatenate nested sequences in outer then inner left-to-right order.
8. `zip` MUST:
   1. emit pairs in left-to-right encounter order,
   2. stop when either input sequence is exhausted,
   3. return tuple pairs whose first element is from `self` and second element is from `u`.
9. `enumerate` MUST emit `(index, value)` tuple pairs in left-to-right encounter order using zero-based `Nat` indices.
10. Tuple pairs returned by `zip` and `enumerate` are accessed through tuple projection syntax from `type-system.md` (`p.0`, `p.1`).
11. `group_by` MUST preserve:
   1. key encounter order in output `Map` iteration,
   2. per-key element encounter order in each grouped `Seq`.
12. `chunk` and `window` size semantics:
   1. if `size == 0`, result MUST be empty sequence,
   2. `chunk` partitions into non-overlapping groups of length `size`, with final remainder chunk allowed,
   3. `window` emits overlapping groups of length `size`; if `size > len(self)`, result is empty sequence.
13. Sequence size/count arguments are `Nat`; signed runtime values MUST be converted with `Nat.from_i64` before use, with failures reported as `DOMAIN_INVALID_VALUE`.
14. The `size == 0` empty-result rule is retained as a compatibility behavior at the `Nat` boundary; negative sizes are not retained as a signed API behavior.


### Effectful Pipeline Combinators (REQUIRED)

These operations are bang and execute callback effects left-to-right.

```eglur-catalog
fn Seq[T].tap!(self: Seq[T], f: callback[effectful](&T) -> Unit) -> Seq[T]
fn Seq[T].for_each!(self: Seq[T], f: callback[effectful](&T) -> Unit) -> Unit
fn Seq[T].map![U](self: Seq[T], f: callback[effectful](T) -> U) -> Seq[U]
fn Seq[T].filter_map![U](self: Seq[T], f: callback[effectful](T) -> Opt[U]) -> Seq[U]
fn Seq[T].find!(self: Seq[T], p: callback[effectful](T) -> Bool) -> Opt[T]
fn Seq[T].find_map![U](self: Seq[T], f: callback[effectful](T) -> Opt[U]) -> Opt[U]
fn Seq[T].any!(self: Seq[T], p: callback[effectful](T) -> Bool) -> Bool
fn Seq[T].all!(self: Seq[T], p: callback[effectful](T) -> Bool) -> Bool
fn Seq[T].partition!(self: Seq[T], p: callback[effectful](T) -> Bool) -> (Seq[T], Seq[T])
fn Seq[T].group_by![K](self: Seq[T], key: callback[effectful](T) -> K) -> Map[K, Seq[T]] requires Eq[K] + Hash[K]
```

Semantics:

1. `tap!` calls `f` for each element and returns the original sequence (or equivalent sequence).
2. `for_each!` evaluates each element in order and returns `Unit`.
3. `map!` applies callback in order and returns a new sequence.
4. `filter_map!` and `find_map!` evaluate callbacks left-to-right.
5. `find!`/`find_map!`/`any!`/`all!` short-circuit with the same stopping rules as pure variants.
6. `partition!` and `group_by!` evaluate callbacks left-to-right and preserve encounter order guarantees from pure variants.

### Fallible `_res` Combinators (REQUIRED)

Pure variants:

```eglur-catalog
fn Seq[T].find_res[E](self: Seq[T], p: callback[pure](T) -> Res[Bool,E]) -> Res[Opt[T],E]
fn Seq[T].find_map_res[U,E](self: Seq[T], f: callback[pure](T) -> Res[Opt[U],E]) -> Res[Opt[U],E]
fn Seq[T].filter_map_res[U,E](self: Seq[T], f: callback[pure](T) -> Res[Opt[U],E]) -> Res[Seq[U],E]
fn Seq[T].partition_res[E](self: Seq[T], p: callback[pure](T) -> Res[Bool,E]) -> Res[(Seq[T], Seq[T]),E]
```

Effectful variants:

```eglur-catalog
fn Seq[T].find_res![E](self: Seq[T], p: callback[effectful](T) -> Res[Bool,E]) -> Res[Opt[T],E]
fn Seq[T].find_map_res![U,E](self: Seq[T], f: callback[effectful](T) -> Res[Opt[U],E]) -> Res[Opt[U],E]
fn Seq[T].filter_map_res![U,E](self: Seq[T], f: callback[effectful](T) -> Res[Opt[U],E]) -> Res[Seq[U],E]
fn Seq[T].partition_res![E](self: Seq[T], p: callback[effectful](T) -> Res[Bool,E]) -> Res[(Seq[T], Seq[T]),E]
```

Normative semantics:

1. All `_res` variants MUST evaluate callbacks left-to-right.
2. All `_res` variants MUST fail-fast at first `Err` and MUST NOT evaluate callbacks at indices `j > i` after first failing index `i` (`TRAVERSE_RES_FAIL`).
3. `_res` failure diagnostics MUST include failing 0-based index (`TRAVERSE_RES_FAIL`).
4. `find_res` and `find_res!` short-circuit on first `Ok(true)` and return `Ok(Some(value))`; when no `true` exists, return `Ok(None)`.
5. `find_map_res` and `find_map_res!` short-circuit on first `Ok(Some(value))`; when no `Some` exists, return `Ok(None)`.
6. `filter_map_res` and `filter_map_res!` evaluate all elements unless fail-fast `Err` occurs.
7. `partition_res` and `partition_res!` evaluate all elements unless fail-fast `Err` occurs and preserve encounter order within both output partitions.
8. Pure `_res` variants MUST reject effectful callbacks (`PURE_COMBINATOR_EFFECTFUL_CALLBACK`).
9. Bang `_res!` variants MAY accept effectful callbacks.


### Sorting and Uniqueness (REQUIRED)

```eglur-catalog
fn Seq[T].sort(self: Seq[T]) -> Seq[T] requires Ord[T]
fn Seq[T].sort_by[K](self: Seq[T], key: callback[pure](T) -> K) -> Seq[T] requires Ord[K]
fn Seq[T].unique(self: Seq[T]) -> Seq[T] requires Eq[T] + Hash[T]
```

Semantics:

1. `sort` and `sort_by` return new sequences.
2. `unique` keeps the first occurrence and preserves encounter order.
3. Sorting MUST be stable.
4. For elements with equal sort key, output order MUST preserve original encounter order.
5. Every element or key comparison MUST dispatch the canonical `std::prelude::Ord.cmp` implementation; sorting MUST NOT use a separate intrinsic or operator-specific ordering.
6. For floating sort keys (`f16`, `bf16`, `f32`, `f64`), comparison MUST follow unified floating total-order semantics from `conventions.md`.
7. For floating sort keys, NaN keys MUST appear after all non-NaN keys.
8. For floating sort keys, signed zero MUST remain ordered as `-0.0 < +0.0`.

### Folds and Scans (REQUIRED)

Pure variants:

```eglur-catalog
fn Seq[A].fold[B](self: Seq[A], init: B, f: callback[pure](B, A) -> B) -> B
fn Seq[A].fold_res[B,E](self: Seq[A], init: B, f: callback[pure](B, A) -> Res[B,E]) -> Res[B,E]
fn Seq[A].scan[B](self: Seq[A], init: B, f: callback[pure](B, A) -> B) -> Seq[B]
fn Seq[A].scan0[B](self: Seq[A], init: B, f: callback[pure](B, A) -> B) -> Seq[B]
```

Effectful variants:

```eglur-catalog
fn Seq[A].fold![B](self: Seq[A], init: B, f: callback[effectful](B, A) -> B) -> B
fn Seq[A].fold_res![B,E](self: Seq[A], init: B, f: callback[effectful](B, A) -> Res[B,E]) -> Res[B,E]
fn Seq[A].scan![B](self: Seq[A], init: B, f: callback[effectful](B, A) -> B) -> Seq[B]
fn Seq[A].scan0![B](self: Seq[A], init: B, f: callback[effectful](B, A) -> B) -> Seq[B]
```

Normative semantics:

1. `fold`, `fold_res`, `scan`, `scan0`, `fold!`, `fold_res!`, `scan!`, and `scan0!` MUST evaluate input elements left-to-right.
2. `fold` over an empty sequence MUST return `init`.
3. `fold!` over an empty sequence MUST return `init`.
4. `fold_res` and `fold_res!` over an empty sequence MUST return `Ok(init)`.
5. `fold_res` and `fold_res!` MUST fail-fast at first `Err` (`FOLD_RES_FAIL`):
   1. if element at 0-based index `i` returns `Err(e)`, result MUST be `Err(e)`,
   2. no callback evaluation for any index `j > i` MAY occur.
6. `fold_res` and `fold_res!` failure diagnostics MUST include failing 0-based index (`FOLD_RES_FAIL`).
7. `scan` and `scan!` MUST exclude the initial seed from output:
   1. output length MUST equal input length.
8. `scan0` and `scan0!` MUST include the initial seed as first output element:
   1. output length MUST equal `input length + 1`,
   2. output element at index `0` MUST equal `init`.
9. `scan` and `scan!` on empty input MUST return empty sequence.
10. `scan0` and `scan0!` on empty input MUST return a singleton sequence containing `init`.
11. Pure variants (`fold`, `fold_res`, `scan`, `scan0`) MUST reject effectful callbacks (`PURE_COMBINATOR_EFFECTFUL_CALLBACK`).
12. Bang variants (`fold!`, `fold_res!`, `scan!`, `scan0!`) MAY accept effectful callbacks.
13. `scan!` and `scan0!` are subject to the module-wide effectful callback timing and replay contract.


### Traverse and Sequence (REQUIRED)

Pure variants:

```eglur-catalog
fn Seq[A].traverse_res[B,E](self: Seq[A], f: callback[pure](A) -> Res[B,E]) -> Res[Seq[B],E]
fn Seq[A].traverse_opt[B](self: Seq[A], f: callback[pure](A) -> Opt[B]) -> Opt[Seq[B]]

fn Seq[Res[A,E]].sequence_res(self: Seq[Res[A,E]]) -> Res[Seq[A],E]
fn Seq[Opt[A]].sequence_opt(self: Seq[Opt[A]]) -> Opt[Seq[A]]
```

Effectful variants:

```eglur-catalog
fn Seq[A].traverse_res![B,E](self: Seq[A], f: callback[effectful](A) -> Res[B,E]) -> Res[Seq[B],E]
fn Seq[A].traverse_opt![B](self: Seq[A], f: callback[effectful](A) -> Opt[B]) -> Opt[Seq[B]]
```

Normative semantics for all variants:

1. Evaluation order MUST be left-to-right.
2. `Res` variants MUST fail-fast at first `Err` (`TRAVERSE_RES_FAIL`).
3. `Opt` variants MUST stop at first `None`.
4. Failure diagnostics:
   1. `Res` fail-fast diagnostics MUST include failing 0-based index (`TRAVERSE_RES_FAIL`),
   2. `Opt` short-circuit diagnostics are optional in baseline v0.1; if emitted, they MUST include failing 0-based index (`TRAVERSE_OPT_NONE`).

### Conversion to Maps and Sets (REQUIRED)

`DuplicateKeyError` in this section is the shared `std::prelude::DuplicateKeyError` type.

```eglur-catalog
fn Seq[K].to_set(self: Seq[K]) -> Set[K] requires Eq[K] + Hash[K]
```

`Seq[(K,V)].to_map` is declared once in `map.md`; this section uses that same callable without redeclaring it.
