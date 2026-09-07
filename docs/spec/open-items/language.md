# Language Open Items

Role: release control. This document owns stable language OPEN-item identities;
it does not define v0.1 language semantics.

Classification authority:

- v0.1 baseline/defer classification for each stable-ID item is machine-tracked in `../open-items-ledger-v0.1.yaml`.
- Release gating consumes ledger summary fields from that file (`../RELEASE-GATE-v0.1.md`).

1. **`LANG-OI-001`** - Whether the core type system standardizes a validation result abstraction such as `Val[T, Seq[E]]`, including its accumulation and evaluation semantics. Public module placement and combinator APIs belong to `STDLIB-OI-001`.
2. **`LANG-OI-002`** - Whether axis identities participate in types beyond the v0.1 surface and which language profiles govern that representation.
3. **`LANG-OI-003`** - Whether core tensor expressions and transform schemas require a closed operator-by-operator axis propagation matrix. Standard-library callable coverage and explicit reshape mapping APIs belong to `STDLIB-OI-003`.
4. **`LANG-OI-004`** - Cancellation and timeout modeling in the language-level `Task` and `Res` semantics, including cancellation state, propagation, and typing. Concrete standard-library operations and deadline policy belong to `STDLIB-OI-004`.
5. **`LANG-OI-005`** - `Sendable` minimal set and serialization policy for process pools.
6. **`LANG-OI-006`** - Public cache-control APIs and general cache enforcement
   beyond the v0.1 hidden-effect and fixed tensor execution-identity rules.
7. **`LANG-OI-007`** - Future module-system extensions beyond v0.1 grammar (for example wildcard imports) and their interaction with current resolution guarantees.
8. **`LANG-OI-008`** - Whether `result` tail-expression return should be added after v0.1.0.
9. **`LANG-OI-009`** - Whether mixed `Res`/`Opt` short-circuiting should be modeled by a lifted sum type.
10. **`LANG-OI-010`** - Whether effectful callable values receive a core type and surface syntax (for example `Fn!(...) -> ...`), including compatibility and invocation rules. Adoption by standard-library callback APIs belongs to `STDLIB-OI-002`.
11. **`LANG-OI-011`** - Whether tensor `contract` gains semiring-generalized source semantics, including type checking, lowering obligations, and interaction with existing transform syntax. The public semiring abstraction and catalog API belong to `STDLIB-OI-005`.
12. **`LANG-OI-012`** - Whether `Tensor[Tri]` becomes an admitted language-level tensor type and, if so, its reduction, branching, and transform semantics. Standard-library operator coverage belongs to `STDLIB-OI-007`.
13. **`LANG-OI-013`** - Whether scoped numeric policy contexts (for example `with numeric(...)`) gain core syntax, scoping, and static semantics after v0.1. Policy values and participating standard-library operations belong to `STDLIB-OI-008`.
14. **`LANG-OI-014`** - Whether explicit capability tokens are standardized to separate effect authorization from `Own[T]` ownership discharge:
   1. define the API surface and typing rules,
   2. keep the extension additive,
   3. preserve v0.1 bang/non-bang behavior when capability features are disabled.
15. **`LANG-OI-015`** - Whether the baseline associated-type model gains generic associated types, defaults, bounds/equality constraints, qualified projection/disambiguation, borrow- or lifetime-qualified associated types, supertraits, or integration with future dynamic trait objects; and whether the finite nominal solver expands beyond its v0.1 boundary. Any borrowed-associated-type extension must define lifetime/scope binding, projection and inference behavior, variance, escape checking, default-method and function-value interaction, diagnostics, and migration from v0.1's reference-free slots.
16. **`LANG-OI-016`** - Whether ADT/enum representation attributes introduce opt-in layout and ABI semantics after baseline v0.x, including supported representation families, eligible types, FFI admissibility, guarantees, and diagnostics. Shared attribute grammar and target integration belong to the Slot D portion of `LANG-OI-019`.
17. **`LANG-OI-017`** - Whether stabilizing `std::diag` requires any core-language integration, such as dedicated syntax, type-system treatment, or effect interaction. The `Diag` payload, `WithDiag` composition, `*_checked` API selection, and preview stabilization criteria belong to `STDLIB-OI-009`; ordinary library-only stabilization does not require a language change.
18. **`LANG-OI-018`** - Test-attribute extension roadmap stabilization ([test-attribute roadmap](../../roadmap/language/test-attributes.md)):
   1. Stage T0 scope (`#[should_err]`, `#[should_runtime_error(...)]`, `#[timeout(...)]`, literal-only `#[case(...)]`),
   2. deterministic reporting contract under parallel execution,
   3. post-T0 decisions (tags, fixtures, generator-based parameterization, and additional expectation attributes).
19. **`LANG-OI-019`** - Shared attribute-surface extensions beyond the v0.1 baseline are tracked in the [attribute roadmap](../../roadmap/language/attributes.md):
   1. custom derive provider registration and expansion surface; external-provider execution authorization belongs to `LANG-OI-020`,
   2. `fn`/`impl` attribute expansion surface,
   3. tensor-device metadata attributes,
   4. grammar and target integration for representation/FFI directives; concrete layout and ABI semantics belong to `LANG-OI-016`.
20. **`LANG-OI-020`** - Whether and when Slot A should move from `locked` (trusted-only provider execution) to `sandboxed` executable external providers, including canonical capability taxonomy and required provenance guarantees.
21. **`LANG-OI-021`** - Whether future profiles add algebraic reducer contracts that permit
    intra-phase reassociation for `reduce{...}.reduce_by`; ordered heterogeneous
    phase-chain typing is closed in v0.1. Named phase syntax belongs to
    `STDLIB-OI-024` and cannot itself authorize reassociation.
22. **`LANG-OI-022`** - Whether a heterogeneous list/container surface (Python-list-like "bucket" semantics) is introduced post-v0.1:
   1. selected candidate direction is ingress-only boxing from the [heterogeneous-sequence roadmap](../../roadmap/language/heterogeneous-sequences.md):
      1. ingress-only ergonomic boxing,
      2. canonical internal representation over `Seq[DynVal]`,
      3. explicit `Res`-based narrowing/extraction,
   2. remaining activation scope is:
      1. exact initial `DynVal` variant set,
      2. feature/profile gating shape,
      3. diagnostic code family and required index payloads,
   3. method/effect/ownership boundaries relative to `Seq[T]` and `Vec[T]`,
   4. compile-time typing vs runtime `Res` validation boundary.
23. **`LANG-OI-023`** - Package lockfile workflow beyond v0.1 baseline requirement:
   1. canonical lockfile update workflow/commands and merge-conflict resolution policy,
   2. lockfile regeneration triggers across multi-package workspaces,
   3. stronger trust and reproducibility profiles beyond baseline canonical-tree
      integrity, such as signatures, transparency evidence, and provenance
      metadata.
24. **`LANG-OI-024`** - Whether an opt-in trusted-pure extern declaration surface is introduced post-v0.1:
   1. syntax/attribute model for explicit trust annotation,
   2. required safety/validation obligations and diagnostics,
   3. interaction with bang suffix/effect classification and conformance profiles.
25. **`LANG-OI-025`** - Whether ownership-capable aggregates and generics are introduced after v0.1:
   1. opt-in linear `struct`/`enum` declaration surface,
   2. ownership-bearing tuple and collection elements,
   3. whole-value and partial-field move rules,
   4. ownership-capable generic parameter and callable-type constraints,
   5. generalization beyond the v0.1 `Opt[Own[T]]` and `Res[Own[T], E]` carrier exceptions.
26. **`LANG-OI-026`** - Typed custom tensor-operation and backend-kernel extension design after v0.1:
   1. source-level operation schemas, namespace identity, input/output shape and dtype contracts, mutation/aliasing, and structured errors,
   2. explicit transform rules for `grad`, future strict `vmap`, export, serialization, and unsupported composition,
   3. a separate backend lowering ABI for device, layout/stride, buffers, multi-input/output, and versioning,
   4. provider discovery, trust, and package integration,
   5. the v0.1 surface has no compatibility requirement for `custom_kernel(name, Tensor) -> Res[Tensor, Str]`.
