# Standard-Library Open Items

Role: release control. This document owns stable standard-library OPEN-item
identities; it does not define v0.1 library semantics.

Classification authority:

- v0.1 baseline/defer classification for each stable-ID item is machine-tracked in `../open-items-ledger-v0.1.yaml`.
- Release gating consumes ledger summary fields from that file (`../RELEASE-GATE-v0.1.md`).

1. **`STDLIB-OI-001`** - If `LANG-OI-001` standardizes validation semantics, define the owning module and public constructors, projections, combinators, and traversal APIs for `Val[T, Seq[E]]`.
2. **`STDLIB-OI-002`** - If `LANG-OI-010` introduces an effectful callable type, define which standard-library callback positions accept it and update their catalog signatures without changing pure-callback contracts implicitly.
3. **`STDLIB-OI-003`** - Extended `std::tensor` callable coverage and explicit reshape mapping APIs. Type-level axis representation and core propagation semantics belong to `LANG-OI-002` and `LANG-OI-003`.
4. **`STDLIB-OI-004`** - If `LANG-OI-004` defines cancellation semantics, specify the concrete cancellation, timeout, and deadline operations beyond the v0.1 structured scope-exit child wait.
5. **`STDLIB-OI-005`** - If `LANG-OI-011` admits semiring-generalized `contract`, define the public semiring abstraction, supplied instances, and catalog-level operation signatures.
6. **`STDLIB-OI-006`** - Whether strict `vmap` introduces dedicated axis-pattern notation in a later version.
7. **`STDLIB-OI-007`** - If `LANG-OI-012` admits `Tensor[Tri]`, define the `std::tensor` operators, reductions, conversions, and catalog signatures available for it beyond scalar `Tri` and `if`-level rules.
8. **`STDLIB-OI-008`** - If `LANG-OI-013` introduces scoped numeric policy contexts, define their policy values, constructors, and the standard-library operations governed by them.
9. **`STDLIB-OI-009`** - `std::diag` library-preview follow-ups (core-language integration, if any, belongs to `LANG-OI-017`):
   1. canonical `Diag` payload shape and compatibility boundary with compile/runtime diagnostic model,
   2. whether `Res[WithDiag[T], E]` remains the only standardized composition or additional canonical forms are added,
   3. pilot `*_checked` API selection and stabilization criteria.
10. **`STDLIB-OI-010`** - Persistent collections preview follow-ups (`std::pvec`/`std::pmap`/`std::pset`):
   1. complexity/profile documentation minimums for structural sharing,
   2. deterministic-order profile boundary (insertion-order-only vs key-ordered alternatives),
   3. conversion policy between mutable and persistent families.
11. **`STDLIB-OI-011`** - `std::prob` RNG-state policy details for explicit non-bang sampling (`sample(self, rng)`), including aliasing/linearity requirements.
12. **`STDLIB-OI-012`** - `std::prob` large-tensor allocation policy for sampling APIs (value-return vs in-place surface such as `sample_into!`) and required effect annotations.
13. **`STDLIB-OI-013`** - If ownership-capable containers are introduced after v0.1, whether their fail-fast traversal APIs consume undispatched suffix values or provide explicit compensating/rollback forms.
14. **`STDLIB-OI-014`** - `std::time` follow-ups:
   1. duration literal suffix surface (`2s`, `150ms`),
   2. richer wall-clock/civil-time types beyond `Instant`/`Duration`,
   3. additional monotonic clock domains, caller-selected host-suspension
      policy, and deadline sleep,
   4. cancellation-aware sleep remains coupled to the post-v0.1 cancellation
      policy in `STDLIB-OI-004`,
   5. if duration literal suffixes are introduced post-v0.1, they MUST preserve the existing `Duration.parse` accepted/rejected grammar, `..`/`..=` range-token precedence, numeric-literal diagnostics outside duration forms, and source validity of constructor/`Duration.parse` calls without rewrite.
15. **`STDLIB-OI-015`** - `std::io::Path` follow-ups:
   1. path literal surface (for example dedicated path literal token),
   2. normalization/canonicalization policy as explicit APIs,
   3. a distinct lossless host-native path carrier and explicit bridge policy
      for native separators, POSIX path bytes, and Windows path code units,
   4. fallible conversion from that carrier to scalar-only `Path` / `Str` and a
      total unambiguous diagnostic rendering,
   5. the future carrier MUST NOT weaken the baseline `Str` Unicode-scalar
      invariant or introduce implicit lossy conversion,
   6. all items above remain deferred non-baseline in v0.1 and MUST NOT be required by v0.1 conformance suites.
16. **`STDLIB-OI-016`** - `std::fp` Reader follow-ups:
   1. representation choice (callable alias vs nominal wrapper) and interoperability boundary,
   2. minimal optional law-check profile scope (Functor/Monad) for preview conformance,
   3. stabilization criteria for possible `std::prelude` re-export.
17. **`STDLIB-OI-017`** - `std::io`/`std::time` diagnostics follow-ups:
   1. whether fine-grained runtime diagnostic-code families (`IO_*`, `TIME_*`) are standardized post-v0.1,
   2. value-channel `IoError`/`PathError`/`TimeError` type taxonomy profile once code families are introduced.
18. **`STDLIB-OI-018`** - Process environment follow-ups:
   1. whether mutation APIs (`set_env!`, `set_cwd!`) are standardized post-v0.1,
   2. whether snapshot includes optional executable path and process identity fields,
   3. host-environment key canonicalization policy beyond v0.1 exact-lookup baseline.
   4. a distinct lossless host-native process-text carrier for argument and
      environment bytes/code units, with explicit scalar conversion and
      diagnostic-rendering boundaries,
   5. mutation APIs and host-native process-text access remain deferred
      non-baseline in v0.1 and MUST NOT be required by v0.1 conformance suites.
19. **`STDLIB-OI-019`** - Text/streaming follow-ups beyond v0.1:
   1. generalized encoding surface beyond UTF-8 (if any) and corresponding error taxonomy expansion,
   2. optional seek/random-access streaming profile boundary,
   3. whether explicit byte-handle adapter constructors are standardized in addition to `open_text_reader!` / `open_text_writer!`,
   4. whether TCP receives an explicit stateful UTF-8 text adapter; any such
      adapter must retain incomplete code points across byte chunks and must
      keep message or line framing separate from TCP chunk boundaries,
   5. direct per-chunk UTF-8 decoding is not a valid replacement for that
      adapter because one code point may span chunks,
   6. if seek/random-access APIs are introduced post-v0.1, they MUST document cursor/position semantics and preserve compatibility with baseline `read_chunk!` EOF + consuming-close rules.
20. **`STDLIB-OI-020`** - `std::concurrency` coordination follow-ups:
   1. channel surface minimum profile (bounded/unbounded semantics, close/drain contracts),
   2. mutex/guard model and poisoning/deadlock policy boundary,
   3. atomics minimum profile and memory-order contract.
21. **`STDLIB-OI-021`** - Future boundary-heavy module candidates:
   1. whether `std::cli` is standardized at all vs left to external libraries,
   2. if standardized, argv/help/exit-code surface and relation to `std::io` standard streams,
   3. whether `std::log` is standardized separately from `std::diag`,
   4. if standardized, sink model, level taxonomy, and deterministic/testing contract,
   5. whether a separate `std::task` module is warranted alongside required `std::concurrency`.
22. **`STDLIB-OI-022`** - `std::json` typed mapping follow-ups:
   1. whether typed encode/decode is standardized at all vs left to external libraries,
   2. if standardized, whether the boundary is trait/protocol-driven, derive-driven, or another explicit schema model,
   3. missing-field, unknown-field, null/default, and duplicate-key projection policy for typed decoding,
   4. confirmation that `Json` remains the canonical interchange model even if typed helpers are added post-v0.1.
23. **`STDLIB-OI-023`** - `Vec` literal ergonomics follow-ups:
   1. whether a dedicated `Vec` literal surface is standardized post-v0.1,
   2. if introduced, whether it uses dedicated literal sugar or another explicit constructor family,
   3. preservation of baseline `[] == Seq[T]` semantics without creating ambiguous container literal typing.
24. **`STDLIB-OI-024`** - `reduce_by` ergonomic follow-ups (algebraic reassociation authority, if added, belongs to `LANG-OI-021`):
   1. whether ordered phase form receives named sugar post-v0.1,
   2. any such sugar MUST preserve ordered phase execution as the canonical internal model for multi-stage reduction customization,
   3. unordered reducer maps/dictionaries remain non-baseline because they obscure phase order for non-commuting reducers.
25. **`STDLIB-OI-025`** - General explicit scalar-cast follow-ups:
   1. whether the source surface is a generic function, conversion-trait method,
      or another explicit form,
   2. the error type and destination-type witness model without introducing a
      reverse dependency from scalar code to `std::tensor::DType`,
   3. explicit scalar `Tri`/`Bool` and numeric conversion policy,
   4. any activated surface must define const/runtime failure-channel parity and
      preserve the checked value-domain rules of `../stdlib/cast-matrix.md` where its
      source and destination domains overlap,
   5. no general explicit scalar-cast API is part of required v0.1 conformance.
