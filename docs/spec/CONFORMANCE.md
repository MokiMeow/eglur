# Eglur Conformance Specification

Status: Normative v0.1.0 released.

Scope: Normative for v0.1 unless this file or a section states otherwise.

## Scope

This document defines how an implementation is judged conformant.

It complements:

- [`SPEC.md`](SPEC.md)
- [`STDLIB.md`](STDLIB.md)
- [`axes.md`](axes.md)
- [`ERRORS.md`](ERRORS.md)
- [`RELEASE-GATE-v0.1.md`](RELEASE-GATE-v0.1.md)

The specification package carries four linked conformance
artifacts:

1. `manifest.yaml` defines which suites are in scope.
2. Suite files define source fixtures and expected outcomes.
3. `probes/registry_cache.yaml` defines implementation-level cache scenarios
   over referenced source fixtures.
4. `traceability.yaml` maps diagnostic obligations to suite evidence.

`artifact-inventory-v0.1.yaml` separately classifies every normative,
conformance-definition, release-control, and generated-evidence artifact. The
spec-package release gate validates the inventory, the four conformance
artifacts, the OPEN-item ledger, and linked machine contracts such as the
standard-library effect catalog as static specification-package inputs. It does
not evaluate an implementation.

Implementation conformance additionally requires a released executable-evidence
profile that binds test-level observations to an exact specification release,
implementation build, and structured execution context. No such profile is
active in the current tooling, so implementation certification is fail-closed
as `UNAVAILABLE`. `RELEASE-GATE-v0.1.md` defines the activation requirements
without prematurely freezing an incomplete evidence schema.

## Suite Discovery

Conformance suites are discovered from:

- `../../tests/conformance/manifest.yaml`

The manifest is authoritative for suite membership and required/optional status.
Suites outside the manifest are not part of v0.1 baseline conformance.
The registry-cache profile is discovered through the artifact inventory and is
not manifest suite membership.

## Diagnostic-code Traceability

The conformance repository MUST maintain a machine-checkable diagnostic-code-to-test
mapping at:

- `../../tests/conformance/traceability.yaml`

Traceability is generated evidence, not a hand-authored substitute for tests.
Change suite cases or diagnostic catalogs first, then regenerate this mapping.
This artifact covers the required diagnostic catalogs listed below.
It is not a complete mapping of every normative `MUST` statement in the language and standard-library specifications.
The generated artifact MUST declare `coverage_scope: diagnostic_codes`, and
spec-package sign-off MUST repeat that exact scope rather than implying broader
normative coverage. A future implementation evidence profile MUST preserve the
same bounded claim.
It MUST also copy the manifest's specification `release` identity and artifact
`status` (`draft` or `released`).
The spec-package gate MUST require the artifact inventory, manifest, generated
traceability, and OPEN-item ledger history roots, release identities, and
statuses to match. A spec-package sign-off is valid only when all four are
`released` and its `specification.release` equals their shared identity.

## Authoritative Artifact Inventory

The repository MUST maintain:

- `artifact-inventory-v0.1.yaml`

The inventory uses three independent classification axes:

1. `role` distinguishes normative specification, conformance definition,
   release control, and generated evidence.
2. `provenance` distinguishes authored artifacts from generated artifacts.
3. `status_source` declares whether release state comes from the inventory,
   the conformance manifest, or the artifact's own top-level `status`.

Every Markdown or YAML artifact under `docs/spec` and every YAML artifact under
`tests/conformance` MUST match exactly one inventory selector. Empty selectors,
overlapping selectors, unclassified files, unsupported classification tuples,
and established artifacts assigned to the wrong role MUST fail the
spec-package gate.

Authoritative artifacts MUST be regular files tracked in the Git index, their
path ancestors MUST NOT be symlinks, and their raw working-tree bytes MUST
equal the indexed blob bytes. An artifact or ancestor symlink, an untracked
file, a tracked artifact missing from the checkout, or bytes hidden from
ordinary Git diff reporting MUST fail validation. Exact lowercase `.yaml` is
the only accepted YAML extension under the authoritative roots; every other
case or `.yml` spelling MUST fail validation.

Artifacts with `status_source: inventory` inherit the inventory status.
Conformance suites with `status_source: manifest` inherit the manifest status.
The registry-cache probe uses `status_source: inventory`; it does not add a
fifth release-state carrier.
Artifacts with `status_source: self` participate directly in the four-way
status agreement above and MUST have a registered status validator. Generated
artifacts MUST have and pass a registered parity validator.

The four state-carrying artifacts MUST each declare the same
`release_history_root` selector, canonical `release` label, and `status`.
The selector is either `repository-root`, covering the unique reachable Git
root and all descendants, or a full lowercase commit ID whose commit and
ancestors contain no release state. The latter covers commits after that
adoption boundary. The selector MUST NOT change within an established
release history. A release label has the form `v0.1.x` or
`v0.1.x-rcN`. Patch and release-candidate numbers have no leading zeroes, and
`N` starts at 1. Their Git-history transitions are closed:

1. Every commit covered by the selector MUST carry complete, agreeing release
   state; `repository-root` includes the parentless initial commit.
2. The initial commit in `repository-root` mode MAY be `draft` or `released`.
   The first identity after an explicit adoption boundary MUST start as `draft`.
3. One `draft` identity MAY remain `draft` or transition to `released`.
4. A `released` identity MUST NOT be reused by a child or prospective commit.
5. One released label MUST bind to exactly one reachable commit.
6. A different identity MUST be later in release order and MUST start as
   `draft`.
7. Release candidates precede the final release of the same patch. A larger
   patch number is later than every identity of a smaller patch, so an
   abandoned release candidate does not require publishing its final release.
8. A merge commit MUST satisfy these rules against every parent, including
   every staged `MERGE_HEAD`.

The gate audits the reachable commit graph covered by `release_history_root`. For a
staged prospective state it additionally compares against `HEAD` and every
`MERGE_HEAD`; once the index matches `HEAD`, the checked-out commit is part of
the audited graph. This keeps the same transition valid both before and after
its commit while preventing state removal, identity laundering, sibling
release duplication, or movement of a released identity to another commit.
The audit MUST use original commit objects and MUST fail when replacement refs,
graft metadata, or shallow history could virtualize or omit ancestry.

Generation/update workflow:

1. Source required diagnostic-code set from:
   - `errors/compile-codes.md`
   - `errors/extension-codes.md`
   - `errors/runtime-codes.md`
2. Map each code to evidence in required suites (baseline conformance-definition scope).
3. Separately collect optional-probe evidence from optional suites (`required: false`) for `optional_emit_if_present` diagnostics.
4. Separately collect optional extension-suite evidence from optional suites (`required: false`) for `excluded_nonbaseline` diagnostics.
5. Classify evidence strength:
   - `asserted` (direct `expect.*.code` assertion),
   - `mentioned_only` (token mention only),
   - `uncovered` (no evidence).
6. Validate field-scoped diagnostic contracts for every structured diagnostic assertion:
   - code and severity sets MUST align between the Markdown catalogs and
     `errors/diagnostic-contracts.yaml`,
   - required-suite message and data assertions MUST equal the portable
     projection of that machine contract,
   - required suites MUST NOT force optional help, related, contextual-message,
     or emit-if-present data behavior.
7. Classify diagnostic policy:
   - `hard_required_asserted` (baseline required; MUST be asserted),
   - `conditional_required` (required only when precondition is active),
   - `optional_emit_if_present` (emission optional; if emitted, token/data contract MUST hold),
   - `excluded_nonbaseline` (extension/non-baseline path; excluded from baseline conformance).

## OPEN-Item Ledger (v0.1 Boundary)

The conformance repository MUST maintain a machine-checkable OPEN-item classification ledger at:

- `docs/spec/open-items-ledger-v0.1.yaml`

The ledger tracks unresolved spec decisions, not suite execution results.
It tells the spec-package release gate whether any v0.1-blocking decisions
remain open.

Authority is split deliberately:

1. work trackers own scheduling, assignment, and execution status;
2. the checked-in OPEN source documents own unresolved decision descriptions;
3. the checked-in ledger owns normative v0.1 release classification;
4. neither a tracker label nor tracker status establishes or clears a normative
   release blocker.

An unresolved decision becomes an accepted v0.1 release blocker only when the
same commit adds both a stable-ID source item and its
`classification: baseline_must_close` ledger entry. Public artifacts MUST use
the stable OPEN ID and semantic description, not local tracker identifiers or
tracker metadata. Work tracking may link to the stable OPEN ID, but release
validation MUST NOT require tracker access.

Ledger policy for v0.1 spec-package release:

1. every top-level OPEN item in:
   - `open-items/language.md`,
   - `open-items/stdlib.md`,
   MUST expose a stable ID and appear exactly once in the ledger `items` under
   the same `source_path` and `id`.
2. list numbers and title text are presentation owned by the source document;
   ledger binding MUST use the stable ID rather than either value.
3. each ledger item MUST have one of:
   - `classification: baseline_must_close`,
   - `classification: deferred_nonbaseline`.
4. the spec-package gate consumes ledger summary fields:
   - `summary.unclassified_count`,
   - `summary.baseline_must_close_open_count`.
5. v0.1 `GO` requires:
   - `summary.unclassified_count == 0`,
   - `summary.baseline_must_close_open_count == 0`.

OPEN-item lifecycle for v0.1:

1. Promotion MUST add the source item and ledger entry in the same commit and
   update the declared source count and derived summary.
2. The resolving commit MUST close the underlying gap and any required
   conformance changes, remove the source item and ledger entry, and update the
   counts.
3. The ledger has no per-item closed state. Every present
   `baseline_must_close` entry is open, so
   `baseline_must_close_open_count` MUST equal the number of those entries.
4. Deferral is not resolution. It MUST follow an explicit baseline-scope
   decision, keep the source item present, and change its classification to
   `deferred_nonbaseline`.
5. Reopening the same semantic decision MUST reuse its stable OPEN ID. A
   materially different decision MUST receive a new ID.
6. The OPEN-item validator MUST reject one-sided source/ledger updates, stale
   counts, unsupported classifications, duplicate IDs, and summaries that do
   not match the classified entries.

For v0.1 static traceability closure:

1. every `hard_required_asserted` code MUST be `asserted`,
2. every `conditional_required` code MUST have an assertion and declared
   precondition binding,
3. `optional_emit_if_present` codes MAY be unasserted, but if emitted they MUST satisfy code/token/data contracts,
4. `excluded_nonbaseline` codes are outside baseline pass/fail.
5. `summary.contract_failure_count` MUST be `0`.

Optional-probe visibility policy for v0.1:

1. Optional-probe aggregation is non-blocking and MUST NOT change baseline hard-gap pass/fail logic.
2. Traceability output MUST include optional-probe asserted/mentioned/unasserted status for `optional_emit_if_present` codes using optional suites.
3. `traceability.yaml` MUST include `optional_probe_summary` fields:
   1. `mode`,
   2. `warning_threshold_min_asserted`,
   3. `warning_active`.
4. `traceability.yaml` `optional_probe_summary` is the canonical source for optional-probe warning state in v0.1.
5. `warning_active` MUST be derived only by threshold comparison:
   1. let `asserted_count = summary.optional_probe_optional_emit_if_present_asserted_count`,
   2. let `threshold = optional_probe_summary.warning_threshold_min_asserted`,
   3. `warning_active == (asserted_count < threshold)`.
6. Runners and spec-package tooling MUST read `warning_active` from
   `traceability.yaml` and MUST NOT recompute it independently from raw suite
   execution logs.
7. For v0.1 baseline, `optional_probe_summary.mode` is fixed to `non_blocking_warning`.
8. For v0.1 baseline, `optional_probe_summary.warning_threshold_min_asserted` is fixed to `1`.
9. Spec-package sign-off MUST raise a warning when
   `optional_probe_summary.warning_active == true`.
10. Reports MUST keep required and optional-probe outcomes separate:
   1. spec-package `GO` is determined by the static release criteria,
   2. a future implementation `PASS` additionally requires complete executable
      evidence for required suites,
   3. optional-probe outcome is warning-only in v0.1 and MUST NOT be merged
      into either hard-fail classification.
11. Spec-package sign-off does not require optional-probe execution evidence.
12. A future implementation evidence profile MUST explicitly include
    test-level optional-probe execution evidence.
13. Missing optional-probe execution evidence MUST prevent implementation
    certification, but does not change the derivation of `warning_active`.
14. Optional-probe coverage MUST remain non-blocking in v0.1.

Optional extension-suite visibility policy:

1. Optional extension-suite aggregation is non-blocking and MUST NOT change baseline hard-gap pass/fail logic.
2. Traceability output MUST include optional extension-suite asserted/mentioned/unasserted status for `excluded_nonbaseline` codes using optional suites.
3. `traceability.yaml` MUST include `optional_extension_summary` and `optional_extension_coverage` sections.
4. `optional_extension_summary.mode` is fixed to `non_blocking_evidence`.
5. Optional extension-suite evidence MUST NOT participate in `optional_probe_summary.warning_active` derivation.


## Required vs Optional Suites

For each entry in `manifest.yaml`:

1. `required: true`
   - the implementation MUST pass the suite
2. `required: false`
   - the implementation MAY skip the suite
3. `precondition: <key>` (optional)
   - when present, suite is required only if `<key>` is active in harness execution context

An implementation is conformant only if all required suites pass.

Precondition activation contract:

1. Harness MUST accept an explicit active-preconditions set.
2. A suite with `precondition: <key>`:
   - MUST run and pass when `<key>` is active,
   - MUST be treated as skipped/N-A (not failure) when `<key>` is inactive.
3. Unknown precondition keys MUST be a harness configuration error.
4. A future implementation evidence profile MUST record the exact
   active-precondition set as a sorted, duplicate-free list and reject any
   mismatch with the execution context.


## Baseline Profiles

v0.1 conformance uses a single baseline language profile for required suites.

Baseline rules:

1. Ownership surface MUST be enabled in baseline conformance mode.
2. Required suites in `manifest.yaml` are evaluated under baseline mode unless a suite explicitly declares extension-profile execution.
3. Implementations MAY expose non-baseline extension modes (including ownership-disabled mode), but these modes are outside required v0.1 conformance unless explicitly listed by extension suites.
4. In ownership-disabled extension mode, ownership syntax/type-surface usage (`Own[...]`, `own`, `move`, `drop_own`) MUST fail with `OWNERSHIP_PROFILE_DISABLED`.

Diagnostics profile naming:

1. Conformance runners MAY enable diagnostics profile `diag-conformance-v1` defined by `errors/ordering.md`.
2. Runners MUST use this exact profile name when exposing profile selection in harness configuration.
3. The profile asserts focused primary diagnostics. It does not require a
   complete emitted diagnostic set, a minimum recovery breadth, or a particular
   diagnostic limit.
4. When an implementation retains multiple compile errors for one fixture, the
   conformance observation MUST project the first error under
   `errors/ordering.md` as its primary diagnostic.
5. Passing required source fixtures is not evidence of multi-error recovery or
   cascade-suppression quality beyond the focused primary assertion.


## Registry Cache Implementation Probe

Registry cache lifecycle conformance is defined separately from source-suite
execution at:

- `../../tests/conformance/probes/registry_cache.yaml`

The artifact has profile name `registry-cache-v1`. It is an authored
conformance definition whose release status comes from the artifact inventory,
not a suite listed by `manifest.yaml`. The spec-package gate MUST decode its
closed schema, resolve every source-fixture reference against the exact
manifest-owned suite bundle, and reject a missing or duplicate scenario,
unknown reference, optional or conditional fixture, unsuitable fixture outcome,
or invalid complete-coordinate relation.

The profile declares `requires_capability: shared_registry_cache`. Static
spec-package validation always validates the profile artifact. Executable
implementation certification applies its cases when implementation metadata
claims that capability or the bound run configuration enables a shared registry
cache. `not_applicable` is valid only when both are absent. The runner MUST
reject `not_applicable` if implementation metadata, run configuration, or the
active cache adapter shows that shared caching is enabled. An implementation
without shared registry caching emits no synthetic case results and remains
eligible for baseline v0.1 certification.

`registry-cache-v1` uses these abstract observations and no others:

1. cache lookup result,
2. cache publication result,
3. provider requests,
4. invocation result, and
5. committed-snapshot visibility.

The profile does not standardize cache paths, receipt bytes, lock files,
filesystem calls, `fsync`, provider-call scheduling, candidate identity, or a
locking or compare-and-publish algorithm. Test-only cache reset, incomplete-state
seeding, fault injection, and concurrency barriers are harness controls, not
Eglur source APIs or portable implementation interfaces.

Case IDs are stable and unique within the profile. Declaration order is not
semantic; decoding orders cases by case ID under canonical lexical text order.

Each case starts in a fresh isolated cache namespace unless its scenario says
otherwise. Every referenced fixture remains one ordinary compiler invocation
and MUST satisfy its source-suite expectation. Its `registry_acquisitions`
table remains immutable and replayable for that invocation. Passing ordinary
source fixtures alone MUST NOT be cited as evidence for cache atomicity,
concurrency, recovery, or physical cleanup.

A successful fixture in this profile has a complete provider acquisition and an
`expect.value` outcome. Its root manifest selector, root lock edge, registry lock
node, acquisition coordinate, target package metadata, and computed canonical
provider-tree digest MUST agree. A provider failure fixture's selected edge,
terminal provider state, diagnostic code, package, and version MUST agree. A
target-package fixture MUST identify the selected registry dependency, and its
computed tree digest MUST match the lock before target validation fails. Every
coordinate-isolation variant that says it reuses the priming tree MUST compute
the same canonical tree digest. The integrity variant's asserted `expected`
digest MUST equal its own locked integrity, and its asserted `actual` digest
MUST equal the computed priming digest.

The profile's source fixtures use only explicit `emit` steps whose decoded paths,
link targets, and contents are exact bytes. Complete-acquisition fixture paths
MUST be ASCII. Static validation applies the normative logical resource limits,
UTF-8 and canonical relative-POSIX path checks, ASCII subset of pinned simple
case folding, duplicate-path checks, and file/ancestor checks before computing a
tree digest. These are fixture restrictions for this lifecycle profile, not a
narrowing of registry package paths accepted by an Eglur implementation.
Every emitted entry receives normative resource accounting before a terminal
provider failure is interpreted. Only a complete acquisition proceeds from that
accounting boundary into path, tree, digest, and target-package validation.

For complete-coordinate isolation, the validator masks only the selected
registry package ID, exact version, and integrity fields in the root manifest,
lock, and provider request. The root source topology, entry source, runner,
remaining manifest and lock facts, and provider-step script MUST otherwise be
identical to the priming fixture.

The scenario oracles are closed:

1. `cold_then_warm`:
   1. start empty and invoke the successful fixture twice in sequence,
   2. the first invocation observes a miss, requests the exact coordinate from
      the provider, commits one snapshot, and satisfies the fixture expectation,
   3. the second invocation observes a hit for that snapshot, makes no provider
      request, performs no new publication, and satisfies the same expectation.
2. `complete_coordinate_isolation`:
   1. run each following variant in a fresh namespace after committing the
      successful priming fixture,
   2. the package variant changes only `RegistryPackageId`, misses, requests its
      exact coordinate, succeeds, and may commit a separately bound snapshot,
   3. the version variant changes only exact version; its provider returns the
      priming tree, so it MUST miss, request its exact coordinate, satisfy its
      target `PACKAGE_LOCK_OUT_OF_SYNC` expectation, and publish no snapshot,
   4. the integrity variant changes only `RegistryTreeIntegrity`; its provider
      returns the priming tree, so it MUST miss, request its exact coordinate,
      satisfy `PACKAGE_REGISTRY_INTEGRITY_MISMATCH`, and publish no snapshot,
   5. the priming snapshot remains committed and visible in every variant.
3. `failed_acquisition_not_published`:
   1. start empty and invoke a fixture that fails with a closed registry
      acquisition outcome,
   2. the invocation observes a miss and provider request, satisfies its source
      expectation, and leaves no committed snapshot for that coordinate.
4. `invalid_package_not_published`:
   1. start empty and invoke a fixture whose acquisition completes and whose tree
      passes integrity comparison but whose target package validation fails,
   2. the invocation satisfies its package diagnostic expectation and leaves no
      committed snapshot for that coordinate.
5. `cache_lookup_unavailable_fallback`:
   1. start empty and inject unavailability only for the lookup operation,
   2. the unavailable result is recorded, the exact coordinate is acquired and
      verified through the ordinary provider boundary, and the invocation
      satisfies the fixture expectation,
   3. publication may succeed, be unavailable, or not be attempted; its exact
      result is recorded and any published snapshot still satisfies the verified
      snapshot lifecycle.
6. `cache_lookup_unavailable_preserves_provider_failure`:
   1. start empty and inject unavailability only for the lookup operation,
   2. request the exact coordinate through the ordinary provider boundary and
      receive the fixture's terminal provider failure,
   3. the invocation satisfies the provider diagnostic expectation, publishes
      no snapshot, and does not replace that outcome with a cache or environment
      failure.
7. `cache_publication_unavailable_fallback`:
   1. start empty, acquire and validate the successful fixture, and inject
      unavailability only at shared-cache publication while retaining the
      usable private verified snapshot,
   2. the publication failure is recorded, no shared snapshot becomes visible,
      and compilation satisfies the fixture expectation from the private
      verified snapshot.
8. `recovered_incomplete_is_miss`:
   1. seed recognizable incomplete state for the successful fixture's
      coordinate before the invocation,
   2. the incomplete state is never visible as a hit; lookup reports a miss,
      ordinary acquisition and publication succeed, and the invocation
      satisfies the fixture expectation.
9. `concurrent_same_coordinate_converges`:
   1. start empty, release the first successful invocation, and suspend its
      publication immediately before the logical commit linearization point
      declared by `publication_pause: before_logical_commit`,
   2. begin the second same-coordinate invocation while publication remains
      suspended and do not resume publication until the harness observes the
      `lookup_rendezvous: second_lookup_entered` event,
   3. after that rendezvous, the second lookup may remain pending or return a
      miss, but MUST NOT return a hit or expose the suspended candidate,
   4. resume publication; one or more provider requests MAY occur and their count
      and order are not asserted,
   5. both invocations satisfy the fixture expectation, exactly one logical
      snapshot becomes committed, and no lookup observes a partial or losing
      candidate.

Spec-package readiness requires the profile artifact and source references to
pass static validation. It does not require a compiler, cache adapter, or probe
execution. A future implementation-certification profile MUST bind the exact
probe artifact bytes, implementation identity, cache-adapter capability, and
structured applicability and per-case observations before claiming these
lifecycle properties.


## Test Case Contract

Each suite file contains `tests: [...]`.

A test case has one source form and one primary expected outcome.
Secondary fields refine that outcome; they do not create another primary result.

Each test case provides:

- `id`
- `category`
- `title`
- `expect`

Each test case MUST provide exactly one source form:

1. Single-file form:
   - `code` (source text for one compilation unit).
2. Single-file raw-byte form:
   - `code_bytes_hex` (even-length hexadecimal source byte string for one compilation unit).
   - This form is for source decoding diagnostics that cannot be represented as UTF-8 YAML text.
3. Multi-file form:
   - `files` (map from relative POSIX path to source text).
   - `symlinks` (optional non-empty map from canonical relative POSIX link path to relative POSIX target).
   - `entry` (relative POSIX path designating a mapped `.eglur` compile entry source; MUST exist in `files`).
   - `file_set_root` (logical source root required only for multi-source file-set mode as specified below).

A package-mode multi-file fixture MAY additionally provide
`registry_acquisitions`, a non-empty list of transactional provider-attempt
scripts. The list is an exact-coordinate provider response table, not an
acquisition history. Each item contains canonical `package`, exact `version`,
and one non-empty `provider_steps` list.

Provider steps are ordered and closed:

1. `emit` contains one logical entry and invokes the compiler-owned sink once.
2. `repeat` contains a positive `i64` `count` and one `emit`. It is equivalent
   to that many consecutive calls to the same sink, not one batch call or a
   precomputed outcome.
3. Exactly one `finish` is present and it is the final step. Its value is
   `complete`, `missing_release`, or `transport_failure`.
4. `finish: missing_release` is the sole step. `finish: complete` MAY follow no
   emissions, and `finish: transport_failure` MAY follow partial emissions.
5. `limit_exceeded` is not a provider step or fixture input. Only the
   compiler-owned sink can produce that acquisition outcome while processing an
   `emit`.

An emitted entry contains one closed `kind` and byte-bearing fields:

1. `file` has exactly one of `path`, `path_hex`, or `path_repeat` and exactly one
   of `text`, `content_hex`, or `content_repeat`.
2. `directory` has exactly one path form.
3. `symlink` and `hardlink` additionally have exactly one of `target`,
   `target_hex`, or `target_repeat`.
4. `block_device`, `character_device`, `fifo`, `socket`, and `other_special`
   have only their path form.

Text forms are exact Unicode scalar text encoded as UTF-8. Hex forms are
canonical lowercase even-length hexadecimal and MAY encode invalid UTF-8 or an
empty byte sequence. Each `*_repeat` form contains exactly one lowercase
`byte_hex` byte and a positive `i64` `count`; it denotes that byte repeated
without requiring the fixture loader to materialize the represented sequence.
These repeat forms are harness instructions, not an Eglur registry wire format
or compiler API type. The harness validates their encoding but deliberately does
not enforce logical acquisition limits, registry-tree path rules, supported tree
kinds, duplicate paths, or link safety.

Each `(package, version)` occurs at most once. Declaration order is not semantic.
Decoding orders acquisitions by `(package, version)` under the
[canonical lexical text order](language/invariants.md#canonical-lexical-text-order).
Within one acquisition, provider-step order is semantic and MUST be preserved in
compilation-input evidence. Text and hex spellings of the same exact bytes
normalize to the same hex evidence. `repeat` and `*_repeat` remain compact
instructions in evidence and are not expanded. Reordering provider emissions
therefore changes the addressed invocation even when both complete scripts would
seal equal entry multisets.

Here compilation-input evidence addresses the complete harness invocation,
including provider operations that may fail before a sealed package source
exists. It does not classify partial sink contents as post-acquisition compiler
source input.

The harness MUST replay the script through the implementation's transactional
provider/compiler boundary. It MUST NOT translate the script directly into a
diagnostic or inject a pre-sealed source:

1. Every `emit`, including each repetition, uses the same fresh compiler-owned
   sink for that attempt.
2. If the sink terminates with `limit_exceeded`, replay stops immediately and no
   later repetition, emission, or `finish` occurs.
3. `finish: complete` seals the emitted entries. Only then does delivery order
   become non-semantic and the complete multiplicity-preserving multiset proceed
   to tree validation.
4. `finish: transport_failure` and every sink failure discard all prior
   emissions. Partial entries cannot participate in tree validation or any later
   compiler phase.
5. No transport container is standardized by the fixture schema.

Provider-table lookup is normative for the harness:

1. A compiler request for one exact `(package, version)` replays the matching
   script from its first step into a fresh sink.
2. A repeated request for the same coordinate replays the same immutable script;
   table entries are not consumed and do not model retries or changing provider
   state.
3. A requested coordinate absent from the table is a harness-configuration
   failure, not an implicit `missing_release` outcome or a compiler diagnostic.
4. A table entry MAY remain unused when compilation terminates before requesting
   it. It remains part of the addressed compiler input but produces no compiler
   observable.

`file_set_root` and `symlinks` MUST NOT be provided with `code` or
`code_bytes_hex`.
`registry_acquisitions` MUST NOT be provided outside package-mode multi-file
fixtures.

`symlinks` describes filesystem topology orthogonally to regular file content:

1. Each link path MUST be a canonical relative POSIX path.
2. Each target MUST be a non-empty relative POSIX path interpreted from the
   link's parent directory. `.` and `..` components are allowed, but lexical
   target resolution MUST remain within the virtual fixture root. Empty path
   components and backslashes are invalid.
3. A link path MUST NOT equal a regular-file path, occur below a regular file,
   contain a regular file below it, or be nested with another link path.
4. A target MAY be nonexistent or participate in a link cycle. Fixture schema
   validation MUST NOT resolve targets or reject those compiler-test states.
5. Harnesses MUST materialize a multi-file fixture in this order: validate the
   fixture; create directories implied by regular files and link parents; write
   every regular file; create every symbolic link without resolving its target;
   then invoke the compiler.

Each test case MAY provide optional harness mode selector:

1. `mode: auto` (default when omitted),
2. `mode: package`,
3. `mode: file-set`.

Harness mode interpretation for multi-file form:

1. If `mode: package` is provided, harness MUST use package mode (`language/package.md`) regardless of whether `files` contains `Eglur.toml`:
   1. if top-level `Eglur.toml` is missing, expected failure code is `PACKAGE_MANIFEST_MISSING`,
   2. if `src/` root is missing, expected failure code is `PACKAGE_SRC_ROOT_MISSING`,
   3. otherwise package-mode validation proceeds under the deterministic order
      from `language/package.md`, including lock structure, synchronization,
      registry tree-domain, and integrity diagnostics.
2. If `mode: file-set` is provided, harness MUST use file-set mode from `language/modules-resolution.md`:
   1. top-level `Eglur.toml`, if present, does not activate package mode,
   2. file-set root validation occurs under the rules below.
3. If `mode` is omitted or `mode: auto`:
   1. if `files` contains top-level `Eglur.toml`, harness MUST use package mode (`language/package.md`):
      1. package root is virtual root of the test file-set,
      2. source root is fixed to `src/`,
      3. dependency visibility is direct-only per manifest `[dependencies]`.
   2. if `files` does not contain top-level `Eglur.toml`, harness uses file-set mode from `language/modules-resolution.md`.

After mode selection, harnesses MUST apply the normative
[file-set invocation contract](language/modules-resolution.md#file-set-invocation-contract)
as follows:

1. Effective package mode MUST reject `file_set_root` as invalid fixture schema.
2. A file-set fixture containing exactly one `.eglur` source MUST omit `file_set_root`; its logical root is the virtual `files` namespace root (`.`).
3. A file-set fixture containing two or more `.eglur` sources MUST provide `file_set_root`.
4. `file_set_root` MUST be `.` or a canonical relative POSIX path.
5. Every `.eglur` source path MUST be at or below `file_set_root`.
   Non-source support files MAY be outside it.
6. The harness MUST pass the declared root unchanged to the file-set build request and MUST NOT infer a replacement from fixture layout, the entry path, or source headers.
7. Violations of Rules 1-5 are fixture/invocation schema failures.
   They MUST be rejected before source compilation and MUST NOT be asserted through `expect.compile_error`.

Each test case MAY provide optional runner selector:

1. `runner: main` (default when omitted):
   1. compile the fixture as an executable invocation,
   2. invoke the entry source's `main!` under
      `language/program-model.md`,
   3. on normal completion, observe the returned `()` of type `Unit` or
      `Res[T, E]` value before process-status projection; a `Never` entrypoint
      has no normal value observation,
   4. use `expect.compile_error`, `expect.runtime_error`, or `expect.value`.
2. `runner: test`:
   1. run the language-level `#[test]`/`#[ignore]` harness defined below,
   2. use `expect.test_run`,
   3. do not require a program `main!`.

`expect` MAY include:

- `compile_error`
- `runtime_error`
- `test_run`
- `value`
- `value_diagnostics`
- `side_effects`
- `process_exit`
- `diagnostic_contains` (optional suites only)

`expect` MUST include exactly one primary outcome key:

1. `compile_error`, or
2. `runtime_error`, or
3. `test_run`, or
4. `value`.

Primary outcome channel selection is normative:

The outcome channel is chosen before diagnostic matching.
Recoverable `Err(...)` and `None` results remain value-channel outcomes.

1. Use `compile_error` when compilation fails and no executable artifact is produced.
2. Use `runtime_error` only for unrecoverable runtime termination (for example trap/panic/fatal runtime diagnostic) where no user-visible return value is produced.
3. Use `value` whenever program execution completes and yields a user-visible value, including `Err(...)`/`None` in `Res`/`Opt`-based fallible APIs.
4. `Res`/`Opt` failure-path conformance for fallible stdlib APIs MUST be asserted under `value` (for example `kind: Err`), not `runtime_error`.
5. If a fallible-path failure is statically decidable under `language/validation-matrix.md`, the test MUST assert `compile_error` instead of `value`, except causes explicitly fixed to value channel in v0.1 baseline (`DETERMINISM_UNAVAILABLE`).
6. In v0.1 baseline suites, `runtime_error` assertions are reserved for runtime-fatal boundaries:
   1. explicit runtime-abort bang paths (for example `std::test::abort!`),
   2. baseline-required implicit runtime-abort boundaries (for example an unobserved child failure at `std::concurrency` scope exit),
   3. non-statically-decidable extern linkage/symbol-resolution failures.
7. Non-bang code paths MUST NOT assert `runtime_error`; these paths are expected to fail at compile time under effect/type rules.
8. A finitely terminating `main!() -> Never` fixture MUST use `runtime_error`,
   not `value`; baseline required suites MUST NOT execute a deliberately
   divergent entrypoint without an independently specified harness bound.

`expect.process_exit` is an optional secondary assertion on `runner: main`:

1. It MUST be integer `0` or `1`.
2. It MUST NOT accompany `compile_error` or `test_run`.
3. With `runtime_error`, it MUST be `1`.
4. With `value`, its required value is determined by the observed entrypoint
   result:
   1. `kind: Unit` or `kind: Ok` requires `0`,
   2. `kind: Err` requires `1`.
5. A `value` assertion using `kind: Unit` and `process_exit: 0` need not add a
   textual `repr`; the pair already observes both the returned value family and
   its process projection.
6. The runner MUST capture this status without treating returned-value
   rendering or stdout/stderr output as the process-result channel.

Diagnostic code source policy is also normative:

1. `compile_error.code` MAY come from:
   1. `errors/compile-codes.md`, and
   2. `errors/extension-codes.md`, and
   3. `errors/runtime-codes.md` when the same cause is statically decidable under `language/validation-matrix.md`.
2. Implementations MUST NOT rename a runtime cause into a compile-only alias when Rule 1.3 applies; cause code identity is preserved (`errors/code-channels.md`).

Primary diagnostic objects use field-scoped matching:

The same base object shape applies to `compile_error`, `runtime_error`, and each
`value_diagnostics` entry:

1. `code` is required.
2. `message_contains` is required and MAY be empty.
3. `message_contains_any` is required exactly when the machine contract defines
   one-of message groups.
4. `data_keys` is required and MAY be empty.
5. `data_equals` is optional. Its keys MUST be listed in `data_keys`, authorized
   by the machine contract's `exact_value_keys`, use only `Str`, signed `i64`,
   or `Bool` values, and match exact scalar kind/value.
6. Required suites MUST derive these fields from
   `errors/diagnostic-contracts.yaml`.
7. Required suites MUST NOT use `help_contains` or `related_contains`.
   Optional suites MAY use them as non-blocking rendering probes.
8. `compile_error` MAY additionally contain exactly one primary-span assertion:
   1. `primary_span: {marker: <name>}`, or
   2. `primary_span: {source: <source>, bytes: {start: <i64>, end: <i64>}}`.
   This field is not allowed on `runtime_error` or `value_diagnostics`.

Compile-error primary markers are source-local assertions:

1. A marker declaration is an end-of-line comment of the form
   `// @primary <name>`.
2. `<name>` MUST be lowercase ASCII kebab case and MUST be unique across all
   Eglur source units (`code` or `.eglur` entries in `files`) in that test case.
3. `primary_span.marker` MUST name exactly one marker declaration.
4. The marker identifies its containing source unit and 1-based line.
   A conforming runner MUST match both against the emitted diagnostic source
   span and its text start.
5. Column and end-position fields are not asserted by this v0.1 surface.
6. Marker comments remain ordinary non-semantic source comments and MUST be
   passed to the compiler with the rest of the fixture source.
7. `code_bytes_hex` fixtures cannot declare or use primary markers.
8. A declared marker without a matching `primary_span.marker` assertion is a
   schema error.

Exact-byte assertions use the canonical source-span coordinate system:

1. `source` MUST identify `code`/`code_bytes_hex` as `code`, or one declared
   `.eglur` entry in `files`.
2. `start` and `end` are non-negative physical byte offsets; `end` is exclusive
   and MUST NOT precede `start` or exceed the source length.
3. A conforming runner MUST match source identity and both offsets exactly.
4. A test MUST NOT combine marker and exact-byte forms.

`side_effects` MAY additionally be used as an ordered list of exact output
strings for secondary assertions.

Top-level `diagnostic_contains` is constrained as follows:

1. Required suites MUST NOT use it.
2. Optional suites MAY use it only when primary outcome is `value`.
3. Its value MUST be a nonempty list of substring strings.
4. It is a rendering probe and MUST NOT substitute for code-scoped diagnostics.

Value assertions support both textual snapshots and numeric semantic comparison:

1. `value`:
   - `kind` (required; for example `Unit`, `Ok`, `Err`, `Some`, `None`),
   - `repr` (optional string-valued textual snapshot),
   - `numeric_compare` (optional typed numeric comparator object),
   - `error_variant` (optional, only when `kind: Err`; type-qualified ADT variant path for the `Err(...)` payload, for example `TextError::InvalidUtf8`).
2. `value_diagnostics`:
   - optional list of structured diagnostics for `value` channel,
   - each entry MUST follow the field-scoped object contract above.
3. `numeric_compare`:
   - `dtype` (required),
   - `shape` (optional for scalar, required for tensor),
   - `expected` (required scalar or flat scalar list in row-major order; for a
     tensor, list length MUST equal the product of `shape`),
   - `mode` (optional; `exact` or `approx`, default `approx` for floating/complex and `exact` otherwise),
   - `rtol`/`atol` (optional overrides; defaults from `stdlib/conventions.md` when `mode=approx`),
   - quoted non-finite real tokens and complex cartesian tokens use the
     destination-dtype grammar from `stdlib/parse.md`; an expected NaN component
     asserts NaN classification, not payload or sign.
4. If both `repr` and `numeric_compare` are present, `numeric_compare` is normative and `repr` is informational snapshot only.
5. `error_variant` is a structured assertion on the error payload discriminant and MUST NOT depend on `to_str`, debug formatting, or payload formatting.
   1. It does not inspect nested payload fields unless a future schema field explicitly adds that contract.
   2. For payload variants with fields, it matches only the outer variant, for example `TextError::Io`.
6. `test_run`:
   - `order` (required complete reported result order),
   - `passed` (required list of passed test references),
   - `skipped` (required list of skipped test references),
   - `failed` (required list of failed test-result objects).
7. Test references inside `test_run.order`, `test_run.passed`, and `test_run.skipped` MUST be structured objects with `module` and `name` string fields.
8. Each `test_run.failed` entry MUST include `module`, `name`, and `kind`:
   - `kind: returned_err` for `Res[Unit, E]` functions returning `Err(e)`,
   - `kind: runtime_abort` for fatal runtime termination; it MUST also include
     `runtime_code`, naming a registered diagnostic code permitted in the
     `runtime_error` channel.
9. Required suites MUST use `repr` normatively only for value families whose textual snapshot is normatively fixed:
   1. `Str` payloads use quoted string-literal snapshots with escaped contents,
   2. nominal error payloads MAY use their canonical nominal type-name snapshot (for example `PathError`, `JsonError`),
   3. user-visible stringification relied upon by required suites MUST come from formatting contracts fixed in `stdlib/prelude.md` and `stdlib/str.md`.
10. `kind: Unit` with `process_exit: 0` is the structured no-snapshot exception
    defined above.


## Evaluation Rules

Evaluation is channel-first: choose the primary outcome, then apply that channel's structured matching rules.

1. Compile-only tests pass when required compile diagnostics match
   (`compile_error.code`, scoped fields, and the marker or exact-byte
   `primary_span` assertion when present).
2. For single-file tests (`code`), harness compiles one virtual source unit.
3. For multi-file tests (`files` + `entry`), harness selects package or file-set mode under the Test Case Contract, applies that mode's root rules, and uses `entry` as compile target.
4. Runtime-error tests pass when `runtime_error.code` and scoped contains fields match rendered runtime diagnostics.
5. Runtime-value tests pass when execution output matches `expect.value`:
   1. non-numeric snapshot checks use `repr`,
   2. numeric/tensor checks use `numeric_compare` when present,
   3. error payload variant checks use `error_variant` when present.
6. Required suites MUST NOT rely on `repr` snapshots for values whose canonical formatting remains unspecified; those tests MUST instead:
   1. assert a more structured outcome,
   2. use `numeric_compare`, or
   3. stringify through a surface whose formatting contract is normatively fixed.
7. If `value_diagnostics` is present, each entry MUST match an emitted value-channel diagnostic by code + scoped fields.
8. Optional rendering probes pass when required substrings in top-level
   `diagnostic_contains` appear.
9. Side-effect tests pass when `emit!` output order equals `expect.side_effects`.
10. When `process_exit` is present, the captured process status MUST equal it
    after the value/runtime channel assertion succeeds.

Substring matching is case-sensitive.


## Completion Progress and Watchdog Boundary

Required finite-progress coverage is carried by these `core/concurrency.yaml`
fixtures:

1. `CON-scope-exit-waits-for-child-tasks-before-return` covers a child whose
   start MAY remain deferred until the scope-exit completion-wait boundary.
2. `CON-explicit-join-success-returns-child-value-through-res` covers an
   explicit `join!` immediately after `spawn!`.
3. `CON-scope-exit-waits-for-all-still-running-child-tasks-before-returning`
   covers multiple unfinished children at one scope-exit boundary.

These fixtures do not make start timing observable. An eager implementation MAY
start a child before the boundary; an implementation that defers start MUST
still complete the same fixture by satisfying the boundary-local progress rule
in `language/concurrency.md`.

Portable source cannot force a greater-index `par_traverse_*` callback to be
in flight when a lower-index fail-fast result becomes decisive: `workers > 1`
is only a maximum worker budget. The conditionally required
`core/par_traverse_quiescence_probe.yaml` suite therefore supplies the
side-effect ordering evidence only when `par_traverse_quiescence_probe` is
active. Its scheduler contract below distinguishes the required quiescent
return boundary from implementation-defined callback interleaving without
turning timing or parallel speedup into baseline requirements.

`CON-nonterminating-child-controls-are-compile-checked-and-skipped` carries two
additional source controls:

1. `blocked_child_control!` contains a child body that joins a nonterminating
   sibling; under a harness-controlled deferred schedule, that child reaches a
   blocked join.
2. `divergent_child_control!` spawns a child that does not terminate; v0.1 does
   not require preemption after that child is selected.

Both controls are language-level `#[ignore]` tests. Their declarations and
bodies MUST compile and type-check as part of the required suite, but the
baseline test runner MUST report them as skipped and MUST NOT execute them.
They are not terminal-outcome conformance cases.
The source alone does not make child start or selection order observable and
MUST NOT be treated as evidence that the blocked state was reached.

Harness watchdog rules:

1. An implementation harness MAY execute the blocked/divergent controls under
   an implementation-specific controlled schedule and an out-of-band watchdog
   as a runner self-test.
2. Watchdog expiration is a harness observation. It MUST NOT be projected as an
   Eglur `value`, `runtime_error`, diagnostic, `process_exit`, or successful
   conformance result.
3. Watchdog expiration while executing a required terminal fixture is a runner
   failure. It does not establish that the program produced a language-level
   timeout result.
4. Executing an ignored nontermination control under a watchdog is harness
   calibration and MUST NOT contribute a passed implementation-conformance
   result.
5. v0.1 does not prescribe one wall-clock watchdog duration. A future active
   implementation-certification profile MUST bind its selected watchdog,
   scheduler-control, and resource policy in immutable run evidence.
6. These harness rules do not introduce the post-v0.1 `#[timeout(...)]`
   language-level test attribute.


## Test-Attribute Harness Contract (v0.1)

This section defines the language-level test runner contract for `#[test]`/`#[ignore]`.

Surface and discovery:

1. Test candidates are top-level function declarations annotated with `#[test]`.
2. Functions additionally annotated with `#[ignore]` are discovered but skipped.
3. Discovery order key is deterministic:
   1. primary key: canonical module/file path under the
      [canonical lexical text order](language/invariants.md#canonical-lexical-text-order),
   2. secondary key: declaration source order within file.

Execution and outcomes:

1. Runner MAY execute non-ignored tests in parallel.
2. Regardless of execution schedule, reported test-result ordering MUST follow discovery order key from this section.
3. A `#[test]` function with return type `Unit` passes when it returns normally.
4. A `#[test]` function with return type `Res[Unit, E]`:
   1. passes on `Ok(())`,
   2. fails on `Err(e)` and MUST surface deterministic failure diagnostics that include rendered `e`.
5. Runtime-abort behavior (`abort!`/panic-equivalent fatal termination) fails the test case.
6. Effect rules are unchanged in test functions:
   1. effectful test bodies still require bang declaration (`fn ...!`),
   2. non-bang tests remain subject to ordinary non-bang effect restrictions.

v0.1 scope boundary:

1. Only `#[test]` and `#[ignore]` are standardized.
2. Additional test attributes are non-normative future work tracked in `../roadmap/language/test-attributes.md`.
3. v0.2+ preview suites MAY exercise Stage T0 (`#[should_err]`, `#[should_runtime_error(...)]`, `#[timeout(...)]`, literal-only `#[case(...)]`) as optional suites (`required: false`), but these are outside required v0.1 conformance.


## Determinism

A conforming implementation MUST produce deterministic results for:

- traversal short-circuit index
- parallel default output ordering (when required by API)
- rendered required diagnostic tokens
- tensor transform structural mapping, dim solving, and segment ownership
  failures (`AXIS_MAPPING_UNSUPPORTED`, `AMBIGUOUS_DIMS`,
  `INCONSISTENT_DIMS`, `DIM_BINDING_REQUIRED`, `SEGMENT_STRADDLE`)
- strict-profile scatter duplicate/update behavior (`SCATTER_SET_DUPLICATE_INDEX` and strict update ordering)
- grad transform shape/dtype and fail-fast behavior

For determinism-sensitive suites, result evaluation MUST include a reported determinism context containing at least:

- compiled artifact identity
- execution plan identity
- compiler build/version
- runtime build/version
- target identity
- backend identifier
- device model
- driver/runtime version
- determinism profile
- worker/thread/process configuration
- randomness configuration

Numeric comparison criteria for default/strict profiles MUST follow `stdlib/conventions.md` exactly.

Determinism-sensitive runtime coverage MUST distinguish three obligations:

1. repeated evaluations of identical inputs under one compiled artifact,
   execution plan, and run-level context compare tensor result and mutable
   tensor post-state bits exactly and require identical error code/data/boundary
   observations;
2. each observation from distinct fixed execution identities compares
   default-profile floating results to deterministic baseline using the dtype
   tolerance and special-value rules;
3. supported strict-profile results compare to the applicable context-bound or
   operation-defined strict baseline bits.

The required `tensor-determinism` fixture returns a two-element `f32` tensor:

1. element 0 is the schedule-sensitive default result; repeated observations
   under one fixed identity compare its payload bits exactly, while distinct
   identities compare it to baseline with `f32` tolerance;
2. element 1 is exactly `+0.0` when the strict call succeeds and exactly
   `2^-30` when it returns `Err`;
3. a `+0.0` marker requires the strict result to match the canonical scatter
   baseline, while a `2^-30` marker is valid only when
   `strict_unavailable_path` is active and the conditional unavailable fixture
   repeats the same normalized operation and passes its exact code/data
   assertions;
4. any other marker or a marker/precondition mismatch is an implementation
   conformance failure.

An exact observer such as `bitcast` is itself exact over its actual input bits.
Cross-identity non-floating differences are permitted only when they are the
deterministic image of permitted upstream floating/complex variation; they are
not an independent tolerance channel. Fallible exact observers MUST retain the
same error outcome/code/data/boundary across those identities.

The run-level execution-context artifact records compiler/runtime and host
facts. Compiled artifact and execution plan identities are test-level facts and
MUST be bound by the future interpreted observed-result payload. Until that
payload is activated, the existing result-binding envelope proves content
binding only and does not by itself establish this determinism obligation.

Conformance checks MUST NOT require a specific primitive execution trace or
contraction planning order unless an operation contract makes logical order
observable. In particular, checked fixed-width integer reductions MUST satisfy
the canonical traversal, left-fold, and first-failure contract in
`stdlib/conventions.md` under every valid physical lowering.

A backend path that cannot provide a fixed, reportable, repeatable execution
plan MUST NOT be used to pass a required default-profile tensor fixture.
Selecting an audited fallback is valid; reporting an uncertified result within
default tolerance is not.

If a run-level context selects active `strict` execution for profile-less APIs,
the implementation MUST establish complete required-primitive support before
the run. It cannot surface missing support through an undeclared per-operation
error union. Explicit profile-bearing strict requests retain the conditional
`DETERMINISM_UNAVAILABLE` path below.

`DETERMINISM_UNAVAILABLE` is conditionally required in baseline:

1. if suite/environment precondition `strict_unavailable_path` is active, tests MUST assert `DETERMINISM_UNAVAILABLE` with required data keys (`profile`, `backend`, `device`);
2. in v0.1 baseline this assertion MUST use runtime value channel (`expect.value.kind: Err` with `expect.value_diagnostics`) and MUST NOT use `expect.compile_error`;
3. required source cases MUST request strict determinism through typed `profile=DeterminismProfile::Strict`, not string profile names;
4. if the precondition is not active, lack of such assertions is not a baseline failure.

`EXTERN_LINKAGE_FAILED` is conditionally required in baseline:

1. if suite/environment precondition `ffi_linkage_probe` is active, tests MUST assert `EXTERN_LINKAGE_FAILED` with required data keys (`symbol`, `abi`) on linkage-failure paths;
2. if the precondition is not active, lack of such assertions is not a baseline failure.

Console write-failure coverage is conditionally required in baseline:

1. if suite/environment precondition `console_write_failure_probe` is active, the harness MUST execute its suite with both stdout and stderr connected to deterministic write-failure sinks while preserving an out-of-band value-result channel;
2. the configured sinks MUST allow standard-stream wrapper acquisition and MUST reject the first non-empty text write with an I/O failure equivalent to a broken pipe;
3. `print!` and `eprint!` cases MUST each return `Err(Io(_))` through the value channel, assert `error_variant: TextError::Io`, and MUST NOT terminate through the runtime-error channel;
4. if the precondition is not active, the suite is skipped/N-A and its absence is not a baseline failure.

Scoped close-failure coverage is conditionally required in baseline:

1. if suite/environment precondition `scoped_close_failure_probe` is active,
   the harness MUST execute its suite with a deterministic byte-writer adapter
   for the reserved `__eglur_conformance_scoped_close_failure_*` paths;
2. the adapter MUST allow acquisition and callback writes to follow the ordinary
   byte-writer contract, return `Err(IoError)` with kind `Other` only from the
   consuming close, and satisfy terminal retirement before that close returns;
3. the harness MUST ensure that the reserved
   `__eglur_conformance_scoped_close_failure_missing.bin` body-error path is
   absent before execution;
4. the suite MUST assert both callback-`Ok`/close-`Err` projection through
   `ScopedFailure::Primary` and callback-`Err`/close-`Err` projection through
   `ScopedFailure::PrimaryAndClose`, including exact payload order and display;
5. if the precondition is not active, the suite is skipped/N-A and its absence
   is not a baseline failure.

Fail-fast parallel-traversal quiescence coverage is conditionally required in
baseline:

1. if suite/environment precondition `par_traverse_quiescence_probe` is active,
   the harness MUST execute its suite under deterministic callback-scheduler
   control;
2. for each quiescence fixture, the controlled schedule MUST:
   1. enter the callback at input index `1` and pause it immediately after its
      `*-in-flight-started` `emit!` marker,
   2. allow input index `0` to produce the decisive `Err` or `None`,
   3. keep input index `2` queued without entering its callback body until the
      decisive outcome is known,
   4. then resume the input-index-`1` callback;
3. the harness MUST NOT activate the precondition unless it can establish that
   schedule without changing callback values or effects other than permitted
   scheduling;
4. the suite's exact side-effect assertions require the in-flight callback's
   completion marker before the caller's post-return marker and forbid the
   queued callback's marker;
5. the runtime-fatal fixture uses the same controlled in-flight state and MUST
   observe `RUNTIME_ABORT` instead of the otherwise decisive typed `Err`;
6. if the precondition is not active, the suite is skipped/N-A and its absence
   is not a baseline failure.


## Tensor Transform Coverage

Required tensor suites MUST include:

- overlay semantics
- primitive elementwise/reduce/movement behavior, including `TensorView` movement
  results and explicit materialization at `Tensor`-only boundaries
- scalar/tensor-shared numeric promotion lattice behavior
- unary transcendental dtype domains, dtype preservation, real IEEE special-value
  behavior, and representative complex principal-branch behavior
- cast legality/value/channel behavior (`CAST_INVALID_VALUE` / `CAST_INVALID_DTYPE_PAIR` and compile-vs-value channel split)
- numeric runtime edge semantics (`INT_OVERFLOW` / `INT_DIV_BY_ZERO` / `INT_REM_BY_ZERO` / `CAST_INVALID_VALUE` / `CAST_INVALID_DTYPE_PAIR`), including promoted-dtype execution, canonical checked integer reduction order, and checked scatter-add rollback
- empty tensor reduction semantics, including additive identities, real/complex
  mean NaNs, empty-axis no-op behavior, `keepdims`, and
  `REDUCTION_EMPTY_DOMAIN` channel selection
- contract behavior
- indexing behavior
- grad transform behavior, including the closed source-active support graph,
  passive-subgraph isolation, reverse-only operation isolation, and the
  lowering-failure versus unsupported-edge diagnostic boundary
- canonical `map_axis` behavior
- transform literals (`view`, `rearrange`, `inflate`, `reduce`, `einop`), with
  `einop` value-level coverage for fresh/output-only axes, repeated literals,
  named and anonymous packs, diagonal inputs, canonical multi-axis reduction,
  schema-static dtype normalization, N-ary promotion, independent multi-output
  evaluation, and expression-atomic failure

For tensor storage-cost contracts, the v0.1 source-fixture surface is
deliberately bounded:

1. Source fixtures observe source-visible carrier types, logical values and
   metadata, mutation isolation, failure behavior, and explicit
   `TensorView.materialize()` boundaries.
2. Those observations do not prove same-buffer construction, absence of
   element-buffer allocation, copy, or `memcpy`, mandatory `REALIZE` placement,
   fresh-buffer ownership, or absence of intermediate materialization.
3. The physical storage and lowering requirements in `axes.md`,
   `language/tensor-model.md`, and `stdlib/tensor.md` remain normative
   implementation obligations, but the current v0.1 source-fixture schema does
   not certify them.
4. A future implementation-certification profile MUST define implementation-level
   lowering or runtime evidence before reporting those obligations as verified.
   Passing a carrier/value fixture alone MUST NOT be cited as zero-copy or
   materialization evidence.
5. v0.1 does not standardize a public source-level storage identity, pointer, or
   alias-inspection API solely for conformance.

Required core suites MUST include:

- lexical/tokenization rules
- parser grammar and precedence rules
- module loading/import/visibility and name-resolution rules
- multi-file module diagnostics for `MODULE_PATH_MISMATCH` and `VISIBILITY_VIOLATION`, including struct-member access and construction boundaries
- core type-checking boundaries (local inference scope, named-function return annotation requirement, generic argument inference behavior, and fixed-width integer literal context/default ordering)
- effects and effect-signature constraints
- explicit unsafe/FFI boundary constraints (`unsafe { ... }`, `extern` declarations, compile-time unsafe-site enforcement, and compile-time dynamic-link effect classification)
- extern ABI subset enforcement (`EXTERN_ABI_UNSUPPORTED`) and conditional linkage-failure probe coverage (`ffi_linkage_probe`)
- ownership surface/profile diagnostics, typed-exit ownership checks, and consuming resource-root close/drop boundaries
- result/opt short-circuit semantics
- trait conflict and dispatch rules
- conditional expression semantics (`if`/`else if`/`match`)
- Tri-aware conditional semantics (`if` with required `unknown` arm for `Tri` conditions)
- loop semantics (`for-in`/`while`/`break`/`continue`)
- network stdlib semantics (`std::net` parse/bind/connect value-channel behavior, scoped `Own` resource cleanup, and bang effect boundaries)
- concurrency semantics (`spawn!` effect boundary, repeatable borrowed `join!`, structured scope-exit wait/observation, and deterministic `par_*` result/index behavior)
- return-statement semantics (`return`/`return <expr>` and scope/type constraints)
- runtime-error channel behavior and compile-vs-runtime separation

## Versioning

`manifest.yaml` and this document version line define the conformance baseline.

If normative behavior changes, version markers MUST be updated together.
