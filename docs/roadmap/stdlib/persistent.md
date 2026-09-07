# Persistent Collections Profile (`std::pvec`, `std::pmap`, `std::pset`) (OPEN, v0.2+)

Status: non-normative design record. This file cannot create v0.1 conformance
requirements.

## Selected Direction

The profile introduces persistent collections as separate optional modules;
they do not replace `Vec`/`Map`/`Set` in the v0.x baseline.

Rationale:

1. keep mutable fast-path ergonomics unchanged for hot update loops,
2. provide explicit immutable structural-sharing tools for branch-heavy/state-snapshot workloads,
3. avoid hidden copy/share heuristics in baseline containers.

## Module Surface (preview)

1. `std::pvec` provides `PVec[T]`.
2. `std::pmap` provides `PMap[K,V]`.
3. `std::pset` provides `PSet[T]`.

## Semantic Contract (preview)

Persistent collections are immutable values with structural sharing.

Rules:

1. non-bang updates return new values and MUST NOT mutate prior versions.
2. implementations SHOULD use structural sharing for asymptotic efficiency.
3. operation semantics MUST be observationally value-equivalent to full-copy immutable updates.
4. deterministic iteration order requirements match baseline container families:
   1. `PVec` preserves index order,
   2. `PMap` and `PSet` preserve deterministic order (insertion-order profile by default, unless future profile states otherwise).

## Candidate Minimal APIs (preview)

`IndexError` and `DuplicateKeyError` in this preview profile are the shared `std::prelude` error types.

### `std::pvec`

```eglur-catalog
opaque type PVec[T]

fn PVec[T].new() -> PVec[T]
fn PVec[T].len(self: &Self) -> Nat
fn PVec[T].is_empty(self: &Self) -> Bool
fn PVec[T].get(self: &Self, i: Nat) -> Opt[T]

fn PVec[T].push(self: PVec[T], x: T) -> PVec[T]
fn PVec[T].insert(self: PVec[T], i: Nat, x: T) -> Res[PVec[T], IndexError]
fn PVec[T].remove(self: PVec[T], i: Nat) -> Res[(PVec[T], T), IndexError]
fn PVec[T].concat(self: PVec[T], other: PVec[T]) -> PVec[T]
fn PVec[T].map[U](self: PVec[T], f: callback[pure](T) -> U) -> PVec[U]
fn PVec[T].filter(self: PVec[T], p: callback[pure](T) -> Bool) -> PVec[T]

fn PVec[T].to_seq(self: PVec[T]) -> Seq[T]
fn Seq[T].to_pvec(self: Seq[T]) -> PVec[T]
fn Vec[T].to_pvec(self: Vec[T]) -> PVec[T]
fn PVec[T].to_vec(self: PVec[T]) -> Vec[T]
```

### `std::pmap`

```eglur-catalog
opaque type PMap[K,V]

fn PMap[K,V].new() -> PMap[K,V]
fn PMap[K,V].len(self: &Self) -> Nat
fn PMap[K,V].is_empty(self: &Self) -> Bool
fn PMap[K,V].contains_key(self: &Self, k: &K) -> Bool
fn PMap[K,V].get(self: &Self, k: &K) -> Opt[V]

fn PMap[K,V].insert(self: PMap[K,V], k: K, v: V) -> PMap[K,V]
fn PMap[K,V].remove(self: PMap[K,V], k: &K) -> PMap[K,V]

fn PMap[K,V].items(self: &Self) -> Seq[(K,V)]
fn PMap[K,V].keys(self: &Self) -> Seq[K]
fn PMap[K,V].values(self: &Self) -> Seq[V]

fn Seq[(K,V)].to_pmap(self: Seq[(K,V)]) -> Res[PMap[K,V], DuplicateKeyError]
fn Map[K,V].to_pmap(self: Map[K,V]) -> PMap[K,V]
fn PMap[K,V].to_map(self: PMap[K,V]) -> Map[K,V] requires Eq[K] + Hash[K]
```

### `std::pset`

```eglur-catalog
opaque type PSet[T]

fn PSet[T].new() -> PSet[T]
fn PSet[T].len(self: &Self) -> Nat
fn PSet[T].contains(self: &Self, x: &T) -> Bool
fn PSet[T].items(self: &Self) -> Seq[T]

fn PSet[T].insert(self: PSet[T], x: T) -> PSet[T]
fn PSet[T].remove(self: PSet[T], x: &T) -> PSet[T]

fn Seq[T].to_pset(self: Seq[T]) -> PSet[T]
fn Set[T].to_pset(self: Set[T]) -> PSet[T]
fn PSet[T].to_set(self: PSet[T]) -> Set[T] requires Eq[T] + Hash[T]
```

## Interop with Baseline Containers (preview)

Explicit conversions are preferred over implicit coercions.

The candidate conversion declarations are grouped with their owning preview modules above.

No implicit promotion between mutable and persistent families in v0.x.

## Performance and Complexity Notes (preview)

1. This profile standardizes semantics, not a single data-structure implementation.
2. Implementations SHOULD document complexity targets and sharing strategy.
3. Conformance for this profile should validate value semantics and deterministic order, not internal node layout.

## Typical Fit

Persistent collections are especially useful for:

1. branch-heavy search/backtracking,
2. compiler/interpreter environment snapshots,
3. undo/redo style state history,
4. concurrent read-mostly snapshots.

Mutable baseline containers remain preferred for tight mutation-heavy hot loops.

## Open Decisions

1. Whether deterministic order for `PMap`/`PSet` remains insertion-order-only or allows a key-ordered profile.
2. Whether future ownership-capable generic profiles should admit owned elements inside persistent collections; direct roots such as `Own[PVec[T]]` need no observable-semantic specialization.
3. Which profile-specific conformance suites are required if/when this module family graduates from preview.
