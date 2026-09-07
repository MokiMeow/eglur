# `std::set` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Type and Determinism

`Set[T]` is a unique-key container whose element domain has explicit equality
and hashing capabilities.

```eglur-catalog
opaque type Set[T: Eq + Hash]
```

Every well-formed `Set[T]` instantiation requires both `Eq[T]` and `Hash[T]`.
The type-head bounds are inherited by every method declaration below and are not
operation-local requirements. Failure to satisfy either bound MUST use
`TRAIT_BOUND_NOT_SATISFIED`.

`Set[T]` implements neither `Eq` nor `Hash` in v0.1. Deterministic iteration
order does not by itself choose between insertion-order-sensitive and
logical-membership equality or hashing.

Iteration order MUST be deterministic and preserve insertion order.


### Constructors and Queries (pure)

```eglur-catalog
fn Set[T].new() -> Set[T]
fn Set[T].len(self: &Self) -> Nat
fn Set[T].contains(self: &Self, x: &T) -> Bool
fn Set[T].items(self: &Self) -> Seq[T]
```


### Pure Value Updates (non-bang)

```eglur-catalog
fn Set[T].insert(self: Set[T], x: T) -> Set[T]
fn Set[T].remove(self: Set[T], x: &T) -> Set[T]
```

Value-update semantics:

1. These methods are pure non-bang operations and MUST NOT mutate the caller-visible source binding.
2. The returned `Set` MUST be observationally independent from the source value: later bang mutation of one value MUST NOT change the other.
3. Implementations MAY copy, move, or share storage internally, but such representation choices MUST NOT be observable.
4. `T` is ownership-free and reference-free in v0.1; ownership-bearing or reference-bearing set elements are outside the baseline value-only generic domain.

Semantics:

1. `insert` MUST return a set that contains `x`.
2. If `x` was absent, `insert` MUST append `x` at the end of insertion order.
3. If `x` was present, `insert` MUST preserve the existing set membership and insertion order.
4. `remove` MUST return a set without `x`.
5. If `x` was absent, `remove` MUST return a set with the same observable membership and insertion order.


### In-place Updates (bang)

```eglur-catalog
fn Set[T].insert!(self: &mut Self, x: T) -> Bool
fn Set[T].remove!(self: &mut Self, x: &T) -> Bool
```

Semantics:

1. `insert!` MUST mutate `self` to contain `x`.
2. If `x` was absent, `insert!` MUST append `x` at the end of insertion order and return `true`.
3. If `x` was present, `insert!` MUST leave membership and insertion order unchanged and return `false`.
4. `remove!` MUST remove `x` from `self` and return `true` when `x` was present.
5. `remove!` MUST leave `self` unchanged and return `false` when `x` was absent.
