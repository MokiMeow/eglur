# `std::prob` (OPEN, v0.2 candidate)

Status: non-normative design record. This file cannot create v0.1 conformance
requirements.

## Scope

`std::prob` is the planned home for first-class probability distributions:

1. `Distribution[T]` value semantics,
2. explicit effect boundary for sampling,
3. pure scoring/log-density APIs,
4. tensor interop for probabilistic ML workloads.

## Candidate Core Surface (OPEN)

```eglur-catalog
opaque type Distribution[T]
opaque type ProbError
opaque type RngState

fn Distribution[T].sample(self: Self, rng: RngState) -> Res[(T, RngState), ProbError]
fn Distribution[T].log_prob(self: Self, x: T) -> Res[f32, ProbError]

fn Distribution[T].map[U](self: Self, f: callback[pure](T) -> U) -> Distribution[U]
fn Distribution[T].flat_map[U](self: Self, f: callback[pure](T) -> Distribution[U]) -> Distribution[U]
```

Notes:

1. Non-bang `sample(self, rng)` is pure and MUST be deterministic for identical `(self, rng)` input.
2. Non-bang sampling MUST require explicit RNG-state injection (no hidden entropy reads).
3. v0.x candidate direction does not include `sample!`; stochastic source state is explicit via `RngState`.
4. `log_prob` is pure.
5. Failure paths are intended to use `Res` and stable `ProbError` diagnostics.

## Tensor Interop Direction (OPEN)

Primary target shape:

1. `Distribution[Tensor[T]]`.

Planned interactions:

1. `map_axis` over sample or batch axes,
2. `grad` over pure scoring paths (`log_prob`, ELBO terms),
3. explicit restrictions for non-reparameterizable sampling paths.

## Open Decisions

1. Minimal distribution set in v0.2 (`Normal`, `Bernoulli`, `Categorical`, etc.).
2. Reparameterization capability surface (trait/marker/API shape).
3. Score-function estimator boundary and required diagnostics.
4. Shape/axis rules for batched distributions and event dimensions.
5. Determinism-profile interaction for stochastic primitives.
6. Whether `Distribution` laws are specified as required algebraic laws in conformance.
7. RNG-state aliasing/linearity policy for pure `sample(self, rng)` threading.
8. Allocation-aware sampling surface (for example in-place `sample_into!`) and its interaction with purity/effect boundaries for large tensors.
