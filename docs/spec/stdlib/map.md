# `std::map` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Type and Determinism

`Map[K,V]` is a key-value container whose key domain has explicit equality and
hashing capabilities.

```eglur-catalog
opaque type Map[K: Eq + Hash,V]
```

Every well-formed `Map[K,V]` instantiation requires both `Eq[K]` and `Hash[K]`.
The type-head bounds are inherited by every method declaration below and are not
operation-local requirements. Failure to satisfy either bound MUST use
`TRAIT_BOUND_NOT_SATISFIED`.

`Map[K,V]` implements neither `Eq` nor `Hash` in v0.1. Deterministic iteration
order does not by itself choose between insertion-order-sensitive and
logical-content equality or hashing.

Iteration order MUST be deterministic. `items()`, `keys()`, and `values()` MUST iterate in insertion order.

`DuplicateKeyError` in this module is the shared `std::prelude::DuplicateKeyError` type.


### Literal Construction

`Map[K,V]` has the source literal form `[k1: v1, ..., kn: vn]`. The empty map
literal is `[:]`; `[]` remains the empty `Seq` literal.

Literal semantics:

1. Each entry evaluates its key expression and then its value expression,
   exactly once each. Entries are evaluated from left to right in source order.
2. Construction is observationally equivalent to starting with
   `Map[K,V].new()` and applying pure `insert(k, v)` once for each evaluated
   entry in source order.
3. Consequently, the first occurrence of a key establishes its insertion-order
   position. A later equal key replaces the stored value and preserves that
   original position.
4. Literal construction is infallible after ordinary expression typing and the
   `Eq + Hash` key bounds have been validated. It does not introduce an
   implicit `Res`, trap, or duplicate-key diagnostic.
5. Code that must reject duplicate keys from a sequence uses the required
   `to_map()` conversion below. Literal construction does not weaken that
   conversion's `DuplicateKeyError` contract.
6. Literal type inference, empty-literal expected context, identifier-key
   interpretation, ownership/reference restrictions, and const eligibility are
   defined in `../language/type-system.md`.


### Constructors and Queries (pure)

```eglur-catalog
fn Map[K,V].new() -> Map[K,V]
fn Map[K,V].len(self: &Self) -> Nat
fn Map[K,V].is_empty(self: &Self) -> Bool

fn Map[K,V].contains_key(self: &Self, k: &K) -> Bool
fn Map[K,V].get(self: &Self, k: &K) -> Opt[V]

fn Map[K,V].items(self: &Self) -> Seq[(K,V)]
fn Map[K,V].keys(self: &Self) -> Seq[K]
fn Map[K,V].values(self: &Self) -> Seq[V]
```


### Pure Value Updates (non-bang)

```eglur-catalog
fn Map[K,V].insert(self: Map[K,V], k: K, v: V) -> Map[K,V]
fn Map[K,V].remove(self: Map[K,V], k: &K) -> Map[K,V]
```

Value-update semantics:

1. These methods are pure non-bang operations and MUST NOT mutate the caller-visible source binding.
2. The returned `Map` MUST be observationally independent from the source value: later bang mutation of one value MUST NOT change the other.
3. Implementations MAY copy, move, or share storage internally, but such representation choices MUST NOT be observable.
4. `K` and `V` are ownership-free and reference-free in v0.1; ownership-bearing or reference-bearing map entries are outside the baseline value-only generic domain.

Semantics:

1. `insert` MUST return a map that contains `k -> v`.
2. If `k` was absent, `insert` MUST append `k` at the end of insertion order.
3. If `k` was present, `insert` MUST replace the existing value and MUST preserve the key's original insertion-order position.
4. `remove` MUST return a map without `k`.
5. If `k` was absent, `remove` MUST return a map with the same observable entries and insertion order.


### In-place Updates (bang)

```eglur-catalog
fn Map[K,V].insert!(self: &mut Self, k: K, v: V) -> Opt[V]
fn Map[K,V].remove!(self: &mut Self, k: &K) -> Opt[V]
```

Semantics:

1. `insert!` MUST mutate `self` so that it contains `k -> v`.
2. If `k` was absent, `insert!` MUST append `k` at the end of insertion order and return `None`.
3. If `k` was present, `insert!` MUST replace the existing value, preserve the key's original insertion-order position, and return `Some(previous_value)`.
4. `remove!` MUST remove `k` from `self` and return `Some(previous_value)` when `k` was present.
5. `remove!` MUST leave `self` unchanged and return `None` when `k` was absent.


### From Sequence (REQUIRED)

```eglur-catalog
fn Seq[(K,V)].to_map(self: Seq[(K,V)]) -> Res[Map[K,V], DuplicateKeyError] requires Eq[K] + Hash[K]
```

Semantics:

1. Duplicate keys MUST return `Err(DuplicateKeyError)` with diagnostic code `DUPLICATE_KEY`.
2. The diagnostic MUST include `key` data when representable and SHOULD include the duplicate key in the message when printable.

Optional convenience:

```eglur-catalog
optional fn Seq[(K,V)].to_map_last_wins(self: Seq[(K,V)]) -> Map[K,V] requires Eq[K] + Hash[K]
```


### Key-ordered Map as Separate Type (`std::ord_map`, OPTIONAL)

`Map[K,V]` is insertion-ordered by design.

Implementations MAY additionally provide a key-ordered map in `std::ord_map`:

```eglur-catalog
opaque type OrdMap[K,V]

fn OrdMap[K,V].new() -> OrdMap[K,V] requires Ord[K]
fn OrdMap[K,V].insert(self: OrdMap[K,V], k: K, v: V) -> OrdMap[K,V]
fn OrdMap[K,V].get(self: &Self, k: &K) -> Opt[V]
fn OrdMap[K,V].items(self: &Self) -> Seq[(K,V)]
```

If `std::ord_map` is provided:

1. iteration order MUST be key order, not insertion order,
2. every key comparison MUST dispatch the canonical `std::prelude::Ord.cmp` implementation,
3. `OrdMap` is a distinct tree-ordered family and does not require `Hash[K]`,
4. the implementation MUST NOT define a representation-specific or separately configurable default ordering.
