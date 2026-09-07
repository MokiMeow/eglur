# Eglur v0.1 Spec-Package Release and Implementation Conformance

Status: Draft evidence contracts; standalone spec-package release is inactive.

Scope: Current availability and artifact validation are normative. Standalone
spec-package sign-off, release-history transitions, and publication procedures
are inactive design material, not requirements on development commits.

## Release Policy

Eglur does not publish a separate specification release. The specification is
a working draft targeting Eglur 0.1.0 and evolves with the implementation.
An eventual integrated release will identify its exact specification revision
and implementation support. Maintainers must establish its acceptance criteria
before enabling a release command; the retained standalone release design does
not establish those criteria.

The four artifact carriers retain `release: v0.1.0` as their target and use
`status: draft`. Their schemas, agreement, and generated-data checks remain
active. A historical `released` marker alone is not evidence that a release
was published, and does not prevent further drafts of this target. The retained
standalone history validator is not a development gate. No public history is
rewritten to change an earlier marker.

Both release CLI modes currently return `UNAVAILABLE` with exit status `2`.
Neither accepts evidence arguments or issues a readiness or certification
verdict. The rules below for a successful standalone `GO`, its history, and
its sidecar apply only to that inactive design. Evidence schemas and static
checks remain useful independently and do not certify an implementation.

## Scope

This document separates three events:

1. **Spec-package release** publishes one immutable revision of the Eglur specification package. Its readiness verdict is `GO` or `NO-GO`.
2. **Implementation conformance certification** evaluates one named implementation in one fully described execution context against one exact released specification package. An active certification profile may issue `PASS` or `FAIL`.
3. **Language or product release** may distribute a compiler, runtime, standard library, and tools. Its release criteria are outside this specification-package policy.

A spec-package `GO` does not claim that a compiler, runtime, conformance run, or conforming implementation exists. An implementation certificate does not itself publish an implementation or declare a language/toolchain product release.

Tools, reports, and CI jobs MUST name the event and verdict they evaluate. They MUST NOT infer a verdict kind from omitted or present evidence arguments.

This policy is complementary to:

1. `CONFORMANCE.md`,
2. `ERRORS.md`,
3. `artifact-inventory-v0.1.yaml`,
4. `tests/conformance/manifest.yaml`,
5. `tests/conformance/probes/registry_cache.yaml`,
6. `tests/conformance/traceability.yaml`.

## Current Certification Availability

The standalone spec-package release gate is inactive under the release policy
above. `pixi run check` remains the supported development quality gate.

The repository does not yet define a complete executable-evidence profile for implementation certification. The CLI therefore reports implementation conformance as `UNAVAILABLE` and exits nonzero. `UNAVAILABLE` is not a conformance verdict.

Until a complete profile is released:

1. no repository artifact can receive `implementation conformance: PASS`,
2. suite-level self-reported `pass` values are not sufficient evidence,
3. arbitrary run IDs, log references, or digests are not sufficient evidence,
4. implementation identity or execution-context placeholders are not sufficient evidence,
5. static specification validation does not claim implementation availability.

This fail-closed state prevents an incomplete evidence schema from being mistaken for implementation certification.

## Spec-Package Release Verdict

This retained design is inactive; the supported CLI cannot issue this verdict.

A specification package is `GO` only if all of the following hold.

1. Authoritative artifact closure:
   1. the artifact inventory, manifest, generated traceability, and OPEN-item ledger satisfy their closed schemas,
   2. the inventory classifies every authoritative Markdown and YAML artifact exactly once,
   3. the four independently stored history roots, release identities, and artifact statuses agree,
   4. Git history satisfies the closed release-identity transition rules,
   5. a `GO` sign-off requires all four statuses to be `released` and the sign-off release identity to match,
   6. the trusted standard-library effect catalog resolves exactly against the indexed baseline callable surface.
2. Conformance-definition integrity:
   1. every manifest suite satisfies the conformance schema,
   2. test IDs and references are valid,
   3. diagnostic assertions do not exceed the portable machine contract,
   4. primary-span markers are present, unique, well formed, and referenced where required,
   5. `registry-cache-v1` has every required scenario and every referenced
      source fixture satisfies its scenario preconditions.
3. Static traceability closure:
   1. `summary.hard_required_gap_count == 0`,
   2. every `hard_required_asserted` diagnostic code has an assertion in a required suite definition,
   3. every `conditional_required` diagnostic code has an assertion for its declared precondition,
   4. `summary.contract_failure_count == 0`,
   5. generated `traceability.yaml` is current.
4. Baseline boundary:
   1. `excluded_nonbaseline` codes remain outside v0.1 baseline pass/fail,
   2. optional suites remain non-blocking,
   3. optional-probe warning state is reported separately from the hard gate.
5. OPEN-item boundary:
   1. checked-in OPEN source documents describe unresolved decisions and
      `docs/spec/open-items-ledger-v0.1.yaml` is authoritative for their v0.1
      classification,
   2. an accepted semantic blocker is projected atomically into those artifacts
      as specified by `CONFORMANCE.md`; tracker state alone is not a release
      input,
   3. `summary.unclassified_count == 0`,
   4. `summary.baseline_must_close_open_count == 0`.
6. Normative prose integrity:
   1. normative keyword checks pass,
   2. the normative baseline documents satisfy the freeze conditions below.
7. Publication evidence:
   1. the sign-off names a concrete v0.1.x release label,
   2. the sign-off names the full clean specification commit,
   3. the sign-off snapshots static coverage and OPEN-item summaries,
   4. the sign-off records `implementation_conformance: not_evaluated`,
   5. the sign-off records `verdict: GO`.

`suite_definitions: VALID` means that suite definitions and their static contracts passed validation. It MUST NOT be represented as suite execution, implementation conformance, or complete coverage of every normative `MUST` statement.

No suite execution result, implementation identity, execution context, or implementation certificate is required for a spec-package `GO`.

## Implementation Profile Activation Requirements

The implementation command MUST remain `UNAVAILABLE` until one released evidence profile defines and validates all of the following.

1. Specification binding:
   1. evidence references a validated spec-package sign-off,
   2. release label, full specification commit, and publication identity agree,
   3. the specification artifact is immutable for the certification run.
2. Implementation binding:
   1. evidence names one implementation and version,
   2. evidence records an immutable build or source revision,
   3. placeholders and unverifiable free-form identities are rejected.
3. Structured execution context:
   1. compiler and runtime revisions,
   2. backend, target, device, and driver/runtime versions where applicable,
   3. the default determinism profile and randomness configuration, plus each
      profile-bearing result's requested and effective profile,
   4. worker, thread, and process configuration where behavior depends on them,
   5. active conformance preconditions.
4. Executable result records:
   1. every required suite and test has an observed result record,
   2. expected and observed outcome channels are compared,
   3. diagnostics, data fields, and primary spans are compared where asserted,
   4. skipped, unsupported, failed, and successful outcomes remain distinct,
   5. missing, duplicate, unknown, or synthesized summary-only records fail validation.
5. Immutable run evidence:
   1. one run identity binds all result records,
   2. result and log artifacts have validated digests,
   3. the validator rejects incompatible context aggregation and mixed runs,
   4. the future run manifest binds the watchdog, scheduler-control, and
      resource-limit policy used for executable fixtures and distinguishes
      harness termination from language-level outcomes.
6. Optional-probe evidence:
   1. optional suite and test IDs are explicit,
   2. execution state and observed outcome are explicit,
   3. evidence binds to the same run identity and immutable artifacts,
   4. static warning state remains separate from execution completeness.
7. Registry cache lifecycle evidence:
   1. evidence binds the exact `registry-cache-v1` artifact bytes separately
      from the source-suite `conformance_bundle_digest`,
   2. evidence binds both implementation capability claims and effective run
      configuration; `not_applicable` is accepted only when neither claims nor
      enables `shared_registry_cache`, and is rejected when the active cache
      adapter or run context shows otherwise,
   3. an accepted `not_applicable` result has no synthetic case results and does
      not fail baseline certification,
   4. when applicable, every profile case has one structured terminal result and
      all five abstract observation classes required by `CONFORMANCE.md`,
   5. the runner identifies the test-only cache adapter capabilities used for
      reset, incomplete-state seeding, fault injection, and concurrency control,
      including the pre-commit publication pause and second-lookup rendezvous,
   6. cold/warm reuse, complete-coordinate isolation, failed-validation
      non-publication, cache-unavailable fallback, recovered-incomplete miss
      behavior, and concurrent convergence are evaluated by their closed
      scenario oracles,
   7. physical paths, receipt bytes, lock files, filesystem-call traces,
      durability mechanisms, provider scheduling, and a particular publication
      algorithm are not required evidence.
8. Tensor storage and lowering evidence:
   1. source-visible carrier, value, metadata, and mutation-isolation results
      remain distinct from physical storage observations,
   2. required zero-copy paths and required materialization paths are verified at
      a standardized implementation-level lowering or runtime observation boundary,
   3. lazy execution, fusion, allocator pooling, backend/device storage, and
      adapter provenance cannot erase or fabricate the required evidence,
   4. the profile MUST NOT require a public source-level storage identity API
      solely to produce conformance evidence.
9. Tensor determinism evidence:
   1. every determinism-sensitive runtime observation binds immutable compiled
      artifact and execution plan identities,
   2. repeated observations under one fixed execution identity and initial
      input compare tensor result and mutable tensor post-state bits
      exactly and compare error code/data/boundary observations exactly,
   3. observations across distinct fixed execution identities each compare to
      default-profile deterministic baseline using dtype tolerances, or to
      applicable strict baseline bits,
   4. the execution plan manifest covers every output-relevant kernel, fusion,
      schedule, math-mode, and dynamic-dispatch choice,
   5. an uncertified backend path cannot be reported as a tolerated
      default-profile result,
   6. the `tensor-determinism` strict-status marker is interpreted against the
      bound active preconditions and the conditional unavailable result for the
      same normalized operation, rather than accepted only by its approximate
      numeric expectation.
10. Adversarial validation:
   1. tests cover forged summaries, placeholder identities, missing records, digest mismatches, and mixed contexts,
   2. tests prove that incomplete evidence cannot produce `PASS`.

Only after these requirements are satisfied may tooling expose an implementation
`PASS` or `FAIL` path. The context-addressing and result-binding envelope below
is frozen. The compile-phase observation envelope below is also frozen.
Runtime/value observation payloads, complete implementation identity, and the
final certificate schema remain unavailable until their activation requirements
are closed.

## Context-Addressed Execution Evidence

Implementation evidence MUST use explicit content addresses rather than
run-level defaults or per-record context overrides. This section freezes the
execution-context and result-binding layer only; satisfying it does not activate
implementation certification.

### Execution Context Artifact

One execution context uses this closed shape:

```yaml
kind: eglur_execution_context
schema_version: '0.1'
context:
  compiler:
    name: egc
    version: 0.1.0
    revision: git:0123456789abcdef0123456789abcdef01234567
  runtime:
    name: eglur-runtime
    version: 0.1.0
    revision: git:fedcba9876543210fedcba9876543210fedcba98
  target:
    triple: x86_64-unknown-linux-gnu
  backend:
    applicability: identified
    name: cpu-reference
    version: 1.0.0
    revision: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  device:
    applicability: identified
    kind: cpu
    vendor: GenuineIntel
    model: reference-model
    ordinal: 0
  driver_runtime:
    applicability: identified
    name: glibc
    version: '2.39'
    revision: sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  determinism:
    default_profile: strict
    randomness:
      mode: fixed_seed
      algorithm: philox4x32-10
      seed: '0'
  parallelism:
    workers: 2
    threads: 4
    processes: 1
  active_preconditions:
    - ffi_linkage_probe
    - strict_unavailable_path
```

Rules:

1. `compiler` and `runtime` names, display versions, and revisions MUST be
   concrete:
   1. `revision` is exactly `git:` plus a full 40- or 64-digit lowercase object
      ID, or `sha256:` plus a 64-digit lowercase content digest,
   2. the all-zero Git null object ID is invalid; object existence and type are
      verified by the future certificate profile,
   3. `TODO`, `TBD`, angle-bracket placeholders, and textual stand-ins such as
      `unknown` are invalid.
2. `target.triple` is the sole target identity. Redundant operating-system and
   architecture fields are not accepted because they could contradict it.
3. `backend` and `driver_runtime` are closed discriminated records:
   1. an applicable component uses exactly `applicability: identified`, `name`,
      concrete display `version`, and immutable `revision`,
   2. an inapplicable component uses exactly
      `applicability: not_applicable`.
4. `device` uses either:
   1. `applicability: identified` with concrete `kind`, `vendor`, `model`, and
      non-negative `ordinal`, or
   2. exactly `applicability: not_applicable`.
5. A missing component is not equivalent to `not_applicable`.
6. `determinism.default_profile` is exactly `default` or `strict` and describes
   the implementation context used by APIs without an explicit `profile`
   argument. A profile-bearing test's requested and effective profile belongs
   to its future test-level result payload; it is not inferred from this field.
7. `determinism.randomness` uses exactly one variant:
   1. `mode: unused`,
   2. `mode: fixed_seed` with concrete string `algorithm` and `seed`, or
   3. `mode: system_entropy` with concrete `source`.
8. `parallelism` records resolved positive `workers`, `threads`, and
   `processes` counts. An implementation default, `auto`, or zero is not an
   observed execution context.
9. `active_preconditions` is sorted, duplicate-free, and contains only
   `console_write_failure_probe`, `ffi_linkage_probe`,
   `par_traverse_quiescence_probe`, `scoped_close_failure_probe`, or
   `strict_unavailable_path`.
10. No field in this context may be inherited from a host, CI job, result
    shard, or sign-off.

### Canonical Content Address

The execution-context address is computed from the complete logical artifact:

1. parse the closed YAML shape above,
2. reconstruct its logical `kind`, `schema_version`, and `context` data,
3. encode canonical JSON with these exact byte rules:
   1. the schema's fixed ASCII object keys are sorted under the
      [canonical lexical text order](language/invariants.md#canonical-lexical-text-order),
   2. array order is preserved,
   3. `,` and `:` have no surrounding whitespace,
   4. JSON strings escape quotation mark and reverse solidus as `\"` and `\\`,
      use `\b`, `\t`, `\n`, `\f`, and `\r` for those five control characters,
      and encode every other `U+0000..U+001F` value as lowercase `\u00xx`,
   5. solidus `/` is never escaped,
   6. all other Unicode scalar values are emitted directly as UTF-8 without
      Unicode normalization; lone surrogate code points are invalid,
   7. integers use base-10 without leading zeroes, while any schema-defined
      booleans or nulls use the exact tokens `true`, `false`, and `null`,
4. hash the UTF-8 bytes with SHA-256,
5. render the address as `sha256:` followed by 64 lowercase hexadecimal digits.

YAML key order, comments, scalar quoting style, and other presentation details
do not change the address. Any logical backend, target, device, driver/runtime,
determinism, randomness, parallelism, compiler/runtime, or precondition change
MUST produce a different address.

### Result Shards

A result shard uses this closed binding envelope:

```yaml
kind: eglur_result_shard
schema_version: '0.1'
records:
  - suite_id: core-effects
    test_id: EFF-pure-call
    run_id: run-2026-07-30-001
    context_digest: sha256:<64-lowercase-hex-digits>
    result_digest: sha256:<64-lowercase-hex-digits>
```

Every record repeats its `run_id` and `context_digest`; there is no shard-level
inheritance or per-record override mechanism. `result_digest` addresses the
structured test-level result artifact below. The result shard itself is
addressed by the same canonical-JSON SHA-256 procedure. Record order remains
significant to that shard address.

### Test-Level Result Artifacts

An executed test result uses one of the `passed`, `failed`, or `unsupported`
states:

```yaml
kind: eglur_test_result
schema_version: '0.1'
suite_id: optional-emit-if-present
test_id: OEP-axis-names-dropped-contract-is-validated-when-emitted
run_id: run-2026-07-30-001
context_digest: sha256:<64-lowercase-hex-digits>
status: passed
observed_result_digest: sha256:<64-lowercase-hex-digits>
log_digest: sha256:<64-lowercase-hex-digits>
```

A test that was not attempted uses the distinct `not_run` state:

```yaml
kind: eglur_test_result
schema_version: '0.1'
suite_id: optional-emit-if-present
test_id: OEP-axis-names-dropped-contract-is-validated-when-emitted
run_id: run-2026-07-30-001
context_digest: sha256:<64-lowercase-hex-digits>
status: not_run
decision_digest: sha256:<64-lowercase-hex-digits>
log_digest: sha256:<64-lowercase-hex-digits>
```

Result rules:

1. `status` is exactly `passed`, `failed`, `unsupported`, or `not_run`.
2. `passed`, `failed`, and `unsupported` require exactly
   `observed_result_digest` and `log_digest`; `decision_digest` is forbidden.
3. `not_run` requires exactly `decision_digest` and `log_digest`;
   `observed_result_digest` is forbidden.
4. `passed` means the test executed and the implementation runner reports that
   all applicable assertions matched.
5. `failed` means execution completed or aborted with at least one asserted
   mismatch or runner failure.
6. `unsupported` means execution was attempted but the named implementation or
   harness could not execute the required path. It is not a pass.
7. `not_run` means no execution attempt occurred. Its decision artifact records
   the immutable scheduling or selection decision; it is not equivalent to
   `unsupported`.
8. The complete test-result artifact receives its own canonical content
   address. A shard record's `result_digest` MUST equal that address, and its
   suite ID, test ID, run ID, and context address MUST equal the artifact.
9. Supplied test-result artifacts and shard references MUST form the same exact
   set. Missing referenced artifacts and unbound extra artifacts are invalid.
10. Every test-result artifact's declared `observed_result_digest`,
    `decision_digest`, and `log_digest` MUST be recomputed from the supplied raw
    artifact bytes. The supplied raw-artifact set MUST match that referenced
    digest set exactly; missing, duplicate, unbound, or content-mismatched
    artifacts are invalid. This rule does not refer to a shard record's
    `result_digest`, which addresses the structured test-result artifact.
11. Every test in a declared optional-probe suite MUST have one result artifact,
    including `not_run` and `unsupported`; omitting an optional result is
    invalid even though the suite remains non-blocking.
12. Validators MUST derive known and optional-probe suite/test pairs from the
    supplied conformance manifest and its exact suite-document set. A
    caller-provided subset or an independently asserted ID list is not accepted.
13. Every referenced suite/test pair MUST exist in that derived inventory.

This layer validates state separation and immutable artifact binding. It does
not yet interpret the raw result artifact, compare expected and observed
channels, authenticate the run, or bind the supplied manifest and suite bytes
to a validated spec-package sign-off. A self-reported `passed` artifact cannot
produce implementation `PASS` until those remaining profile requirements are
active.

For a determinism-sensitive runtime fixture, the future interpreted raw result
artifact MUST additionally bind the compiled artifact and execution plan
manifests used by each observed evaluation. Those test-level identities are not
run-level execution-context fields: one conformance run may compile many
fixtures and may intentionally compare independent artifacts or plans. This
requirement does not extend the frozen test-result binding envelope above;
`observed_result_digest` content-addresses the interpreted payload that will own
these bindings.

The interpreted payload MUST also group repeated observations under one
content-addressed activation manifest that binds the fixed artifact, plan, and
run-level context before evaluation. The future certificate profile must bind
that group to one ordered physical evaluation sequence; content addressing
alone does not prove uninterrupted process provenance. A profile-less active
strict context is admissible only when its activation manifest certifies every
required primitive path before execution.

### Sign-off Binding

The context/result component of an eventual implementation sign-off uses this
closed shape:

```yaml
kind: eglur_implementation_evidence_binding
schema_version: '0.1'
run_id: run-2026-07-30-001
context_digest: sha256:<64-lowercase-hex-digits>
result_shard_digests:
  - sha256:<64-lowercase-hex-digits>
```

Binding rules:

1. `result_shard_digests` MUST be non-empty, sorted, and duplicate-free.
2. The supplied execution-context artifact MUST have exactly the named
   `context_digest`; unbound context artifacts are rejected.
3. The supplied result-shard set MUST match `result_shard_digests` exactly.
   Missing and unbound extra shards are rejected.
4. Every shard record MUST resolve to one exact test-result artifact under the
   test-level rules above.
5. Every result record MUST repeat the binding's exact declared `run_id` and
   `context_digest`.
6. A `(suite_id, test_id)` pair MUST occur at most once across all bound shards.
7. One sign-off binding represents one declared run ID in one execution
   context. Backend, device, driver/runtime, default determinism profile,
   randomness, worker, thread, process, or active-precondition differences
   require a different context address and a separate sign-off binding.
8. Validators MUST NOT declare two context addresses compatible, normalize
   differences away, or aggregate them under one implementation verdict.
9. This layer rejects declared run-ID mismatches; it cannot prove that shards
   from separate physical executions were not relabeled with one run ID.
10. Content addressing proves binding consistency, not artifact provenance or
   authenticity. The future certificate profile MUST bind this envelope to an
   immutable run manifest and publication evidence before `PASS` is available.

`tests/conformance/tools/eglur_conformance/evidence/execution/` is the reference
implementation for this layer: `model.py` defines canonical values, `codec.py`
parses and addresses artifacts, `projection.py` derives the suite inventory, and
`policy.py` validates cross-artifact bindings.

## Compile-Phase Observation Evidence

Compilation is an independently observable phase. A successful compile or
typecheck MUST NOT be represented as whole-test `passed`: most positive
fixtures still require a later `value`, `runtime_error`, or `test_run`
observation. Compile evidence therefore uses separate artifacts and a separate
exact-set binding.

### Compiler Result Transport

Interactive compiler use SHOULD remain human-readable by default. Machine
integration is opt-in and emits one `eglur_compile_result` JSON document. Raw
stdout and stderr are not a third presentation mode: the conformance harness
captures the exact byte streams from that same machine-mode invocation.
The harness MUST NOT run a fixture once for human output and again for
structured output.

This specification does not standardize a compiler executable name or option
spelling. A compiler integration MAY either:

1. emit the result document natively, or
2. use an implementation-specific adapter that emits the result document.

An adapter is part of the evidence-producing toolchain, not the compiler
result. Its concrete version and immutable revision MUST be bound by the future
run-manifest and certificate profile before implementation `PASS` is available.

A successful result uses this closed JSON shape:

```json
{
  "kind": "eglur_compile_result",
  "schema_version": "0.1",
  "outcome": {
    "kind": "succeeded"
  }
}
```

A source rejection uses the same portable primary-diagnostic projection as the
compile observation:

```json
{
  "kind": "eglur_compile_result",
  "schema_version": "0.1",
  "outcome": {
    "kind": "rejected",
    "primary_diagnostic": {
      "severity": "error",
      "code": "NAME_NOT_FOUND",
      "message": "name `missing` was not found",
      "data": {
        "name": {
          "kind": "str",
          "value": "missing"
        }
      },
      "primary_span": {
        "kind": "source",
        "source": "code",
        "bytes": {"start": 8, "end": 15},
        "text": {
          "start": {"line": 1, "column": 9},
          "end": {"line": 1, "column": 16}
        }
      }
    }
  }
}
```

Transport rules:

1. raw stdout MUST contain at most 1,048,576 bytes,
2. stdout MUST contain exactly one UTF-8 JSON document and optional surrounding
   JSON whitespace; a byte-order mark, a second document, duplicate object
   keys, non-JSON numeric constants, or non-whitespace text is invalid,
3. JSON nesting depth MUST NOT exceed 32, counting the top-level value as depth
   1,
4. JSON object keys and values follow the closed shapes above; presentation
   whitespace and object-key order are not constrained,
5. `succeeded` requires normal process exit code `0`,
6. `rejected` requires normal process exit code `1`,
7. a signal, any other exit code, malformed result document, exceeded transport
   limit, or disagreement between the result and compile observation invalidates
   the evidence,
8. stderr is an uninterpreted raw log byte stream; it is content-addressed but
   MUST NOT be parsed as the compiler result,
9. the complete raw stdout and stderr bytes are retained and addressed exactly;
   validators MUST use one immutable byte snapshot for both hashing and parsing,
   and the JSON document MUST NOT contain a digest of its own stdout bytes,
10. this document is the stable compile-phase certification projection, not a
    streaming build-event or complete IDE diagnostic protocol; v0.1 does not
    define JSON Lines output.

### Compile Observation Artifact

A successful compilation uses this closed shape:

```yaml
kind: eglur_compile_observation
schema_version: '0.1'
suite_id: core-effects
test_id: EFF-bang-function-may-call-print
specification_commit: 0123456789abcdef0123456789abcdef01234567
run_id: run-2026-07-30-compile-001
context_digest: sha256:<64-lowercase-hex-digits>
input_digest: sha256:<64-lowercase-hex-digits>
compiler_result_digest: sha256:<64-lowercase-hex-digits>
log_digest: sha256:<64-lowercase-hex-digits>
termination:
  kind: exited
  code: 0
outcome:
  kind: succeeded
```

A source rejection uses `outcome.kind: rejected` and one structured primary
error:

```yaml
kind: eglur_compile_observation
schema_version: '0.1'
suite_id: core-modules
test_id: MOD-unresolved-name-is-rejected
specification_commit: 0123456789abcdef0123456789abcdef01234567
run_id: run-2026-07-30-compile-001
context_digest: sha256:<64-lowercase-hex-digits>
input_digest: sha256:<64-lowercase-hex-digits>
compiler_result_digest: sha256:<64-lowercase-hex-digits>
log_digest: sha256:<64-lowercase-hex-digits>
termination:
  kind: exited
  code: 1
outcome:
  kind: rejected
  primary_diagnostic:
    severity: error
    code: NAME_NOT_FOUND
    message: name `missing` was not found
    data:
      name:
        kind: str
        value: missing
    primary_span:
      kind: source
      source: code
      bytes:
        start: 8
        end: 15
      text:
        start: {line: 1, column: 9}
        end: {line: 1, column: 16}
```

`primary_span` uses exactly one variant:

1. `kind: source` with a non-empty `source`, canonical non-negative half-open
   physical `bytes`, and optional derived `text` coordinates,
2. exactly `kind: unavailable`.

The source and byte range are canonical. A `text` projection, when present,
uses positive 1-based Unicode-scalar coordinates after leading-BOM handling
and logical line-ending normalization. Invalid UTF-8 diagnostics use a
byte-only source span; they MUST NOT use `unavailable` or include `text`.

Diagnostic `data` values preserve scalar kind explicitly:

1. `kind: bool` has a JSON boolean `value`,
2. `kind: int` has a signed 64-bit JSON integer `value` and does not accept
   booleans,
3. `kind: str` has a Unicode-scalar string `value`, including the empty string.

`null`, floating-point values, and integers outside the signed `i64` range are
invalid. This tagged projection is exactly the `Str | i64 | Bool` diagnostic
data domain in `errors/model.md`; tags prevent host-language boolean/integer
coercion from changing evidence identity.

Lone surrogate code points are invalid in all evidence strings. No Unicode
normalization is applied.

### Compilation Input Address

`input_digest` addresses the exact logical conformance invocation, including the
compiler and its harness-owned boundary adapters, using the
canonical JSON and SHA-256 rules defined for execution-context artifacts. Its
payload contains:

1. exact `suite_id` and `test_id`,
2. `mode`, expanding omission to `auto`,
3. `runner`, expanding omission to `main`,
4. exactly one source variant:
   1. `utf8` with the exact `code` string,
   2. `bytes` with `code_bytes_hex` decoded and re-emitted as lowercase
      complete-byte hexadecimal,
   3. `files` with the exact relative-path-to-text mapping, the exact
      relative-link-path-to-target `symlinks` mapping (empty when omitted),
      `entry`, and `file_set_root` as either its explicit string or `null`.
5. `registry_acquisitions` is always present as a list. Each item contains
   canonical `package`, exact `version`, and ordered `provider_steps`.
   1. `emit` includes one canonical entry. Exact byte sources use lowercase
      `path_hex` and kind-specific lowercase `content_hex` or `target_hex`.
      Compact byte sources instead preserve their `path_repeat`,
      `content_repeat`, or `target_repeat` descriptor with lowercase `byte_hex`
      and exact `count`.
   2. `repeat` preserves its exact positive `count` and canonical nested `emit`.
   3. `finish` preserves `complete`, `missing_release`, or
      `transport_failure`.
   Acquisition order compares `(package, version)` field-by-field under the
   [canonical lexical text order](language/invariants.md#canonical-lexical-text-order).
   Provider-step order is preserved. YAML mapping-key order and equivalent
   text-versus-hex spelling are not semantic. Compact repetition is not expanded
   in the addressed payload.

Mapping key order does not affect the address. Source bytes, source text,
paths, link paths or targets, registry coordinates, provider scripts, entry
bytes, the designated source entry path, declared mode, runner, suite ID, or
test ID changes do.
The input projection does not parse source text to infer type correctness.

### Compile Comparison

The validator derives compile expectations only from the validated conformance
manifest and its exact suite-document set:

1. suites with `required: false` are outside this compile evidence profile,
2. a required suite without `precondition` is active,
3. a required suite with `precondition` is active exactly when the named
   precondition appears in the bound execution context,
4. an active fixture with `expect.compile_error` is a negative compile fixture,
5. every other active fixture is a positive compile fixture, regardless of
   source spelling.

Every active required fixture MUST have exactly one observation. Missing,
duplicate, optional, inactive, or unknown suite/test identities are invalid.
A positive fixture MUST use `succeeded`; a negative fixture MUST use `rejected`.

For a rejected fixture, the structured primary diagnostic is compared by the
field-scoped rules in `errors/rendering.md`:

1. severity is exactly `error`,
2. code matches exactly,
3. every `message_contains` token occurs case-sensitively in `message`,
4. every `message_contains_any` group contributes at least one matching token,
5. every declared `data_keys` key exists; additional emit-if-present data keys
   are allowed,
6. every `data_equals` key matches exact tagged scalar kind and value,
7. when `primary_span.marker` is declared, its source unit and line match the
   source span and its text start,
8. when `primary_span.source` plus `bytes` is declared, both match the source
   span's source identity and physical byte range exactly.

`compiler_result_digest` addresses the exact stdout bytes from machine mode.
`log_digest` addresses the exact stderr bytes from the same process.
`termination` records either normal `exited` with a non-negative integer
`code`, or `signaled` with a positive integer `signal`. The complete streams
remain separate raw artifacts. Their SHA-256 addresses MUST be recomputed from
the supplied bytes, and the supplied raw-artifact set MUST equal the referenced
set exactly. Identical streams from separate invocations share one
content-addressed raw artifact.

### Compile Evidence Binding

One compile phase uses this closed binding:

```yaml
kind: eglur_compile_evidence_binding
schema_version: '0.1'
specification:
  release: v0.1.0
  commit: 0123456789abcdef0123456789abcdef01234567
  spec_package_signoff_digest: sha256:<64-lowercase-hex-digits>
  conformance_bundle_digest: sha256:<64-lowercase-hex-digits>
run_id: run-2026-07-30-compile-001
context_digest: sha256:<64-lowercase-hex-digits>
observation_digests:
  - sha256:<64-lowercase-hex-digits>
```

Binding rules:

1. `observation_digests` is non-empty, sorted, and duplicate-free.
2. Supplied observation artifacts and `observation_digests` form the same exact
   set.
3. Every observation repeats the binding's exact specification commit, run ID,
   and context address.
4. The supplied execution context exactly matches `context_digest`; compiler
   name, version, and immutable revision in that context identify the compiler
   build.
5. The supplied spec-package sign-off bytes exactly match
   `spec_package_signoff_digest`; their release, commit, and
   `conformance_bundle_digest` match the binding.
6. `conformance_bundle_digest` addresses the exact manifest bytes and the exact
   path-to-SHA-256 mapping of all suite-document bytes. The supplied manifest and
   suite-document byte set MUST reproduce that address before it is parsed.
7. The addressed suite-document paths match the manifest exactly, and the
   conformance manifest release matches the binding release.
8. A summary count or aggregate status cannot replace per-fixture observations.
9. Compile success is only a compile-phase match. It does not create or imply a
   whole-test result or implementation-conformance verdict.

Under the inactive standalone design, the sign-off artifact would have to pass
the spec-package gate before
an eventual implementation certificate may trust it. This layer parses the
standard `eglur_compile_result` transport and checks it against raw stream
addresses, termination state, and compile observations. It does not invoke a
compiler, authenticate a physical run, or authenticate an adapter.
Consequently, synthetic or self-reported observations cannot activate
implementation `PASS`; the final run manifest and certificate profile remain
fail-closed.

`tests/conformance/tools/eglur_conformance/evidence/compilation/` is the
reference implementation for this phase: `model.py` defines canonical values,
`codec.py` parses and addresses artifacts, `projection.py` derives compile inputs
and fixture expectations, and `policy.py` performs field-scoped comparison and
exact-set validation.

## Artifact Inventory and Freeze Conditions

`docs/spec/artifact-inventory-v0.1.yaml` is the authoritative freeze boundary.
It classifies:

1. normative Markdown and machine-readable contracts,
2. conformance manifest, source-suite definitions, and implementation-probe
   definitions,
3. release-control artifacts,
4. generated traceability evidence.

Classification uses independent `role`, `provenance`, and `status_source`
fields. Authored and generated artifacts remain distinct even though both are
frozen at the exact released specification commit. Files whose status comes
from `inventory` or `manifest` inherit that artifact's state; files whose
status source is `self` participate directly in release-state agreement.

The inventory, manifest, generated traceability, and OPEN-item ledger each
store the same `release_history_root`, `release` identity, and `status`.
`release_history_root` selects one of two origins:

1. The exact literal `repository-root` selects the unique parentless commit
   reachable from the candidate and any prospective merge heads. That commit
   MUST carry complete, agreeing release state and MAY start as `draft` or
   `released`. Every reachable descendant MUST also carry complete release
   state. Multiple reachable Git roots are invalid.
2. A full lowercase Git commit ID selects an existing adoption boundary. That
   commit and its ancestry MUST contain no release state; every reachable
   commit after it MUST carry complete release state. The first identity
   after this boundary MUST start as `draft`.

The selector MUST remain unchanged throughout the release history. The
`repository-root` literal is resolved from committed Git objects, not replaced
with a commit's own hash in its files. A new repository can therefore start
with one complete release snapshot. Its sign-off is created after that commit
and records the resulting exact commit ID in a sidecar artifact. An uncommitted
initial tree cannot receive a successful sign-off.

A canonical identity is `v0.1.x` or `v0.1.x-rcN`, with no
leading zeroes and `N >= 1`.

The inventory validator MUST reject:

1. an authoritative Markdown or YAML file matched by no selector,
2. a file matched by more than one selector,
3. an empty or out-of-scope selector,
4. an unsupported classification tuple,
5. an established artifact assigned to the wrong semantic role,
6. a self-status artifact without a registered status validator,
7. a generated artifact without a registered parity validator,
8. an untracked, missing, or symlinked authoritative artifact or path ancestor,
9. authoritative working-tree bytes that differ from the indexed blob,
10. a YAML extension other than exact lowercase `.yaml`.

Freeze policy:

1. Every inventoried artifact is frozen at the exact commit named by a
   successful spec-package sign-off.
2. Generated artifacts MUST be current with their authored inputs at that
   commit.
3. The trusted standard-library effect catalog MUST resolve exactly against the
   baseline callable surface, including own summaries, callback positions, and
   callable re-exports.
4. Every commit included by the selected origin carries complete, agreeing
   release state, including the initial commit in `repository-root` mode.
5. A newly introduced identity starts as `draft`, except for the parentless
   initial snapshot in `repository-root` mode described above.
6. The same identity MAY transition only from `draft` to `released`; it MAY
   remain `draft` across intermediate commits.
7. A commit after a released identity MUST introduce a strictly later identity
   in `draft`; neither `released -> draft` nor `released -> released` reuse is
   valid for the same identity.
8. A released label binds to one reachable commit only, including across
   merged branches.
9. Release-candidate identities sort before the final identity of the same
   patch, and patch numbers increase monotonically; moving to a larger patch
   does not require publishing the final form of an abandoned candidate.
10. Every merge parent, including each staged `MERGE_HEAD`, is checked
   independently.
11. Replacement refs, graft metadata, and shallow repositories are forbidden;
    history is evaluated from original commit objects.
12. Editorial clarifications, preview sections, and OPEN sections may evolve
   only under the same identity and transition rules; semantic broadening of
   baseline `MUST` behavior after freeze therefore begins a later draft
   identity.

## Diagnostic Stability Expectations

For Eglur Specification v0.1.x maintenance releases:

1. Required diagnostic `code` identifiers are stable.
2. Severity class (`error`, `warning`, or `note`) is stable for required codes.
3. Required field-scoped contracts in `errors/diagnostic-contracts.yaml` are stable:
   1. `message_contains`,
   2. `message_contains_any`,
   3. `data_keys`,
   4. `data_equals` where authorized for a required suite.
4. `related` remains optional to emit, but its `RelatedSpan` field shape,
   coordinate convention, end semantics, and ordering contract are stable when
   emitted.
5. Exact full prose strings are not frozen; implementations may improve wording while preserving required token contracts.

## Optional Emit-if-present Policy

`optional_emit_if_present` diagnostics are represented by optional probe suites and do not block the v0.1 spec-package verdict.

Current optional probe suite:

1. `tests/conformance/optional/emit_if_present.yaml`.

Static spec-package policy:

1. `tests/conformance/traceability.yaml` is the canonical warning-state source.
2. Warning state is derived by:
   1. `asserted_count = summary.optional_probe_optional_emit_if_present_asserted_count`,
   2. `threshold = optional_probe_summary.warning_threshold_min_asserted`,
   3. `warning_active == (asserted_count < threshold)`.
3. The validator MUST read `optional_probe_summary.warning_active` and MUST NOT recompute it independently.
4. v0.1 fixes:
   1. `optional_probe_summary.mode == non_blocking_warning`,
   2. `optional_probe_summary.warning_threshold_min_asserted == 1`.
5. An active warning MUST be recorded in the spec-package sign-off.
6. The warning MUST NOT flip spec-package `GO` to `NO-GO` while hard criteria pass.
7. No optional-probe execution evidence is required for a spec-package release.

Implementation certification requires the structured optional-probe evidence described by the activation requirements. No free-form optional-probe evidence field is accepted while certification is unavailable.

## Spec-Package Sign-off Format

A spec-package release sign-off uses this closed shape:

```yaml
kind: spec_package_release
specification:
  release: v0.1.0-rc1
  commit: <full-specification-sha>
coverage_scope: diagnostic_codes
artifact_inventory: docs/spec/artifact-inventory-v0.1.yaml
manifest: tests/conformance/manifest.yaml
conformance_bundle_digest: sha256:<64-lowercase-hex-digits>
traceability: tests/conformance/traceability.yaml
open_items_ledger: docs/spec/open-items-ledger-v0.1.yaml
suite_definitions: VALID
hard_required_gap_count: 0
conditional_required_asserted_count: <n>/<n>
optional_suites: non-blocking
optional_probe_optional_emit_if_present_asserted_count: <n>
optional_probe_warning_active: true | false
optional_probe_warning_note: <warning-text-or-n/a>
optional_probe_warning_threshold_min_asserted: 1
open_items_unclassified_count: 0
open_items_baseline_must_close_open_count: 0
implementation_conformance: not_evaluated
verdict: GO
approver: <name>
timestamp_utc: <UTC>
```

`conformance_bundle_digest` uses the conformance-suite bundle-addressing rule:
canonical JSON and SHA-256 over the exact manifest-byte digest and the exact
suite-path-to-byte-digest mapping. The spec-package gate MUST compute it from
the clean indexed candidate checkout and reject any different sign-off value.
This makes the sign-off, rather than a later compile-evidence author, the trust
anchor for the source-fixture inventory. It does not include
`probes/registry_cache.yaml`; that artifact is frozen by the inventory and exact
specification commit, and future implementation evidence MUST bind its bytes
separately.

The supported command reports availability only:

```bash
pixi -q run --locked release-gate spec-package
```

It writes the following status to stderr and exits `2`:

```text
spec-package release readiness: UNAVAILABLE
```

The implementation command currently fails closed:

```bash
pixi -q run --locked release-gate implementation
```

Its output is:

```text
implementation conformance: UNAVAILABLE
```

## CLI and CI Boundary

`tests/conformance/tools/validate_release_gate.py` is the canonical executable
boundary and the locked `release-gate` Pixi task is its supported invocation.
Argument parsing is owned by
`eglur_conformance/spec_release/cli.py`; the inactive standalone orchestration
is retained in `eglur_conformance/spec_release/gate.py`. Integrations invoke the
Pixi task rather than importing the top-level script as a Python API.

1. The validator requires the `spec-package` or `implementation` subcommand.
2. `spec-package` MUST report `UNAVAILABLE` and exit `2`. It accepts no sign-off, repository-root, or execution-evidence arguments.
3. `implementation` MUST exit nonzero and report `UNAVAILABLE` until a complete executable-evidence profile is released.
4. Argument omission MUST NOT select a weaker verdict.
5. Neither current subcommand can return a successful readiness or certification verdict.
6. CI MUST NOT present static development checks as a spec-package release approval.
7. CI MUST NOT publish a green implementation-conformance job while certification is unavailable.
8. CI summaries MUST NOT shorten a verdict to an unqualified `release gate`, `GO`, `PASS`, or `green`.
9. `coverage_scope: diagnostic_codes` attests only the diagnostic catalogs named by `traceability.yaml`; it does not claim complete normative-`MUST` coverage.
