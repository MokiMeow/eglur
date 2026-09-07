# Attribute Extension Roadmap (v0.2+)

Status: non-normative design record. This file cannot create v0.1 conformance
requirements.

This document records staged attribute-extension slots after v0.1 attribute-lite.

v0.1 conformance baseline excludes all extension slots in this file.

Stable release-control ownership is split by concern in
`../../spec/open-items/language.md`: `LANG-OI-019` owns the shared attribute
surface, `LANG-OI-020` owns external-provider execution authorization, and
`LANG-OI-016` owns representation layout and ABI semantics.

## Cross-slot Preservation Constraints

Any future attribute extension in this roadmap MUST preserve:

1. deterministic expansion and deterministic diagnostics ordering.
2. source-origin primary spans for user-facing diagnostics.
3. purity/effect rules from `../../spec/language/effects.md` unless an extension explicitly revises them.
4. coherence/orphan/overlap re-checking for generated `impl` output.

## Slot A: Custom Derive Provider Model

Status: `OPEN`\
Earliest target: v0.2+

Scope:

1. registration/discovery of user-provided derive providers.
2. deterministic provider resolution and expansion phase.
3. provider failure diagnostics.

Baseline boundary:

1. v0.1 baseline remains closed-set (`../../spec/language/algebraic-data.md`): built-in + implementation-trusted providers only.
2. Slot A defines the post-v0.1 path to registration-enabled provider model.

Planned model (when Slot A is enabled):

1. Registration surface:
   1. providers are registered at package level via manifest metadata (not source-level registration statements),
   2. registration key is derive trait path (`TraitPath`),
   3. derive syntax remains `#[derive(TraitPath, ...)]` (no separate provider-name syntax).
2. Resolution policy:
   1. mapping `TraitPath -> Provider` MUST be unique in active manifest/profile,
   2. duplicate provider registrations for one `TraitPath` MUST fail deterministically at compile setup with `DERIVE_TRAIT_UNSUPPORTED`,
   3. source-level disambiguation forms (for example `derive(T by P)`) are not part of Slot A baseline.
3. Expansion phase ordering:
   1. expansion runs after path/symbol resolution and before full type/coherence validation,
   2. deterministic traversal order MUST be fixed as:
      1. module path order,
      2. declaration source order within module,
      3. derive-entry order within one attribute list.
4. Purity and capability boundary:
   1. provider execution MUST be deterministic and side-effect free,
   2. provider execution MUST NOT depend on wall-clock time, randomness, process/environment probes, or file/network I/O unless a future capability profile explicitly revises this.
5. Expansion boundary:
   1. provider output is impl-only: `impl Trait for AnnotatedType`,
   2. provider output MUST NOT define new nominal types, free functions, constants, modules, or impls for non-annotated receiver types.
6. Coherence and validation:
   1. generated impls MUST go through ordinary global coherence/orphan/overlap/duplicate checks with no derive-specific exemption,
   2. generated impl conflicts reuse existing coherence diagnostics rather than introducing derive-specific coherence-code forks.
7. Generic and bound policy:
   1. provider may use annotated type's existing generic parameters,
   2. provider may add only minimal required bounds needed to type-check generated impl,
   3. provider MUST NOT add unrelated arbitrary bounds.
8. Eligibility diagnostics:
   1. payload/bound unsatisfied cases SHOULD map to `DERIVE_ELIGIBILITY_FAILED`,
   2. expansion-boundary violations SHOULD map to `DERIVE_EXPANSION_FORBIDDEN`.
9. Execution profile and capability model:
   1. Slot A activation MUST define an extension execution profile.
   2. Baseline profile is `locked`:
      1. external user-provided providers are not executable,
      2. only built-in and implementation-trusted providers MAY execute.
   3. In `locked`, non-trusted provider registrations MUST fail deterministically during compile setup with source-primary diagnostics.
   4. Optional profile `sandboxed` MAY be introduced later:
      1. provider runtime MUST be sandboxed (for example WASM-like constrained runtime),
      2. provider host capabilities default to minimal set (`ast_read`, `emit_impl`),
      3. filesystem/network/process/environment/time/random capabilities remain denied unless a future capability revision explicitly permits them.
   5. Any executable provider profile MUST be reproducibility-pinned:
      1. provider identity includes versioned artifact hash,
      2. deterministic mode requires fixed provider identity set for compilation.
   6. Any executable provider profile MUST enforce deterministic resource boundaries (for example fixed fuel/memory budgets) and deterministic failure diagnostics on exhaustion.
10. Purity/effect contract under Slot A profiles:
   1. profile changes MUST NOT alter language-level bang/non-bang effect typing,
   2. generated outputs remain subject to ordinary purity/effect checks after expansion.

Open sub-questions for Slot A activation:

1. Minimum required trusted-provider provenance metadata (signature vs hash-only).
2. Exact capability taxonomy if `sandboxed` is enabled (single-level allowlist vs tiered capabilities).

v0.1 non-goals:

1. user-provided derive providers are not part of required baseline.
2. derive remains limited to v0.1 supported families and built-in semantics.
3. built-in derives for `Display`, `From`, `TryFrom`, and `Error` are deferred. The callable `Display.to_str` contract is fixed by `std::prelude`; remaining derive work concerns structural formatting generation and eligibility, conversion generation, and error-display conventions.

## Slot B: `fn` and `impl` Attribute Surface

Status: `OPEN`\
Earliest target: v0.2+

Scope:

1. semantic attributes on function declarations.
2. semantic attributes on `impl` blocks/items.
3. expansion-time admissibility and rejection diagnostics.

v0.1 non-goals:

1. no required semantic attribute families on `fn`/`impl` beyond baseline test attributes and derive constraints already specified.

## Slot C: Tensor-device Metadata Attributes

Status: `OPEN`\
Earliest target: v0.2+

Scope:

1. backend/device selection metadata.
2. kernel/lowering hint metadata.
3. reproducibility and determinism interactions.

Planned staged model:

1. Stage `C0` (minimal built-in attribute-lite surface):
   1. `#[cfg_device(...)]`:
      1. semantic target: `fn` and `impl` declarations only,
      2. purpose: device/backend applicability filtering for candidate selection,
      3. no semantic authority to bypass type/effect/coherence checks.
   2. `#[determinism(...)]`:
      1. semantic target: `fn` and `impl` declarations only,
      2. values: `default` or `strict`,
      3. purpose: local determinism profile requirement for enclosed lowering/execution paths.
   3. `C0` excludes kernel-tuning directives and codegen pragmas.
2. Stage `C1` (post-`C0`, optional):
   1. `#[kernel_hint(...)]` family (advisory-only lowering hints),
   2. conflict policy with strict determinism must be explicit and deterministic.
3. Determinism contract for Slot C:
   1. `#[determinism(strict)]` on a selected path requires strict-profile availability;
   2. unavailable strict path MUST fail with `DETERMINISM_UNAVAILABLE` (no silent fallback).
4. Diagnostics policy for Slot C:
   1. in v0.1 baseline, these attributes remain unsupported and continue to fail with `ATTRIBUTE_UNSUPPORTED`,
   2. when Slot C is enabled, unsupported targets/arguments/conflicts MUST have deterministic diagnostics and source-primary spans.
5. Purity/effect interaction:
   1. Slot C attributes are metadata constraints only,
   2. Slot C MUST NOT reclassify non-bang code as effectful or vice versa.
6. Implementation phase boundary:
   1. Slot C applicability filtering runs after symbol/path resolution and before full lowering planning,
   2. this ordering MUST be deterministic under module-path and source-order traversal rules.

Open sub-questions for Slot C activation:

1. Precedence between item-level `#[determinism(...)]`, call-site execution context, and package/profile defaults.
2. Canonical selector vocabulary for `cfg_device` (standardized names vs implementation profiles).
3. Whether dedicated compile-channel error codes should be added for Slot C target/argument conflicts, or continue using `ATTRIBUTE_UNSUPPORTED`-family umbrella codes.

v0.1 non-goals:

1. no required device-selection attribute syntax.
2. no required kernel-hint attribute syntax.

## Slot D: Representation/FFI Attributes

Status: `OPEN`\
Earliest target: v0.2+

Scope:

1. opt-in representation/layout controls (`repr`-like families).
2. ABI/FFI boundary contracts for attributed types.
3. diagnostics for unsupported or conflicting representation directives.

v0.1 non-goals:

1. enum/ADT layout remains unspecified by default.
2. no required `repr(...)` attribute semantics.
