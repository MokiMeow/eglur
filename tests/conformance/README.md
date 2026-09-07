# Conformance Suite Author Guide

This guide covers suite discovery, fixture authoring, test IDs, traceability,
and validation commands. The normative contract is the [Eglur conformance
specification](../../docs/spec/CONFORMANCE.md). Tool maintainers should use the
[tooling architecture guide](tools/README.md); release operators should use the
[release and evidence guide](RELEASE.md).

## Suite Discovery

`manifest.yaml` is the canonical entry point for source-suite discovery.
`probes/registry_cache.yaml` is the separate `registry-cache-v1`
implementation-probe definition; it is not a source suite. Its executable cases
apply to an implementation run that claims or enables `shared_registry_cache`.

Manifest suite entries support:

1. `required: true|false`
2. optional `precondition: <key>` for conditionally-required suites

Precondition semantics:

1. if suite precondition is active in harness context, the suite is required;
2. if inactive, the suite is skipped/N-A and does not fail baseline conformance;
3. unknown precondition keys are harness configuration errors.

Recognized v0.1 preconditions are `strict_unavailable_path`,
`ffi_linkage_probe`, `console_write_failure_probe`,
`scoped_close_failure_probe`, and `par_traverse_quiescence_probe`. The console
probe runs its suite with stdout and stderr connected to deterministic
write-failure sinks while preserving an out-of-band value-result channel for
the harness. The scoped-close probe routes reserved byte-writer paths through
the deterministic close-failure adapter defined by
`docs/spec/CONFORMANCE.md`. The parallel-traversal probe uses the controlled
callback schedule defined there; ordinary `workers > 1` execution does not
activate it.

Diagnostics profile:

1. Harnesses MAY expose `diag-conformance-v1` from `docs/spec/errors/ordering.md`.
2. If profile selection is exposed, this exact profile name MUST be used.
3. Compile-error fixtures compare one ordered primary diagnostic. They do not
   assert that the compiler emitted exactly one error or recovered far enough to
   discover every independent error.
4. A compiler observation with multiple retained errors MUST project the first
   error under the normative ordering. The evidence schema carries only that
   projection.

## Fixture Authoring

### Source Forms

Each test uses exactly one source form:

1. Single-file:
   - `code: | ...`
2. Single-file raw bytes:
   - `code_bytes_hex: "..."` with an even-length hexadecimal byte string
   - intended for source decoding diagnostics that cannot be represented as UTF-8 YAML text
3. Multi-file:
   - `files:` mapping relative paths to source text
   - optional non-empty `symlinks:` mapping canonical relative POSIX link paths to relative POSIX targets
   - `entry:` compile entry `.eglur` source path (must be present in `files`)
   - `file_set_root:` logical source root required only for file-set mode with two or more `.eglur` sources

Package-mode multi-file fixtures may add `registry_acquisitions:`:

1. Coordinates are unique. Every item has canonical `package`, exact `version`,
   and ordered `provider_steps`.
2. `emit` sends one closed entry variant.
3. `repeat` sends one entry a positive number of times.
4. One final `finish` is `complete`, `missing_release`, or `transport_failure`.
5. Text, canonical lowercase hex, and compact repeated-byte forms encode exact
   path, content, and link-target bytes. No archive encoding is implied.
6. The harness replays every emission through the compiler-owned sink; only
   that sink may produce `limit_exceeded`.

`registry_acquisitions` is a provider response table rather than a history.
Repeated lookup of one coordinate replays the same immutable script into a fresh
sink. A requested coordinate with no table entry is a harness error, while an
entry may remain unused when an earlier normative compilation failure prevents
its request.

### Registry Cache Profile

`probes/registry_cache.yaml` references manifest-owned single-invocation
fixtures and supplies the closed multi-invocation cache scenarios. The profile
covers cold/warm reuse, complete-coordinate isolation, failed-acquisition and
invalid-package non-publication, lookup and publication fallback, recovered
incomplete state, and same-coordinate concurrent convergence.

`validate_conformance_schema.py` and the spec-package gate validate the profile
schema; the gate does not execute probe cases. Those cases run only under an
implementation-certification profile. Cache paths, receipts, lock files,
filesystem calls, durability mechanisms, and provider scheduling are outside
the portable probe contract.

### Harness and Runner Selection

Optional harness mode selector:

1. `mode: auto` (default when omitted)
2. `mode: package`
3. `mode: file-set`

Mode behavior:

1. `mode: package` forces package-mode compilation.
2. `mode: file-set` forces file-set compilation.
3. `mode: auto` selects package mode when top-level `Eglur.toml` exists in `files`, else file-set mode.
4. In package mode (`mode: package` or `mode: auto` with top-level `Eglur.toml`), v0.1 baseline requires top-level `Eglur.lock`; missing lockfile paths assert `PACKAGE_LOCK_MISSING`.
5. Package mode forbids `file_set_root`.
6. A file set with exactly one `.eglur` source omits `file_set_root` and uses implicit root `.`.
7. A file set with two or more `.eglur` sources requires `file_set_root`.
8. `file_set_root` is `.` or a canonical relative POSIX path; every `.eglur` source must be at or below it.
9. Invalid root configuration is a fixture schema failure, not `expect.compile_error`.

`symlinks` is valid only with `files`. Link targets are interpreted from each
link's parent and may use `.` or `..` only while remaining lexically inside the
fixture root. Link paths cannot conflict or nest with regular files or other
links. Missing targets and link cycles are valid fixture topology so compiler
diagnostics can cover them. A harness creates implied directories, writes all
regular files, creates links without resolving their targets, and only then
invokes the compiler.

Optional runner selector:

1. `runner: main` (default): compile an executable invocation, run the entry
   source's zero-argument `main!` under
   `docs/spec/language/program-model.md`, observe its normal `Unit`/`Res` result
   when one exists, and use `expect.compile_error`, `expect.runtime_error`, or
   `expect.value`. A `Never` entrypoint has no value outcome; a finitely
   terminating fixture uses `runtime_error`.
2. `runner: test`: run the language-level `#[test]`/`#[ignore]` harness defined
   in `docs/spec/CONFORMANCE.md`, use `expect.test_run`, and do not require
   `main!`.

### Outcome Assertions

Primary outcome assertions are one-of:

1. `expect.compile_error`
2. `expect.runtime_error`
3. `expect.test_run`
4. `expect.value`

Optional secondary diagnostics for value channel:

1. `expect.value_diagnostics` (structured, code-scoped; preferred)
2. `expect.diagnostic_contains` (optional-suite-only rendering probe)

Optional process-status assertion for `runner: main`:

1. `expect.process_exit: 0` accompanies `value.kind: Unit` or `value.kind: Ok`.
2. `expect.process_exit: 1` accompanies `value.kind: Err` or `runtime_error`.
3. It is invalid with `compile_error`, `test_run`, or `runner: test`.
4. `value.kind: Unit` plus `process_exit: 0` does not require a `repr`.

Choose primary outcome channel by runtime behavior:

1. `compile_error`: compile fails.
2. `runtime_error`: unrecoverable runtime abort/trap (no returned user value).
3. `test_run`: runner-mode report for discovered `#[test]` functions.
4. `value`: execution returns a value, including `Err`/`None` via `Res`/`Opt`.

For fallible stdlib APIs modeled as `Res`/`Opt`, assert failure with `expect.value.kind` (`Err`/`None`) rather than `runtime_error`.

Code identity across channels:

1. `compile_error.code` may use:
   1. compile-channel codes from `docs/spec/errors/compile-codes.md`,
   2. extension-profile compile-channel codes from `docs/spec/errors/extension-codes.md`,
   3. runtime cause codes from `docs/spec/errors/runtime-codes.md` when statically decidable.
      1. v0.1 exception: `DETERMINISM_UNAVAILABLE` is fixed to value channel and is not asserted via `compile_error`.
2. Do not introduce per-channel aliases for the same failure cause.

For runtime-error channel checks, use explicit unrecoverable runtime paths (for example `std::test::abort!`) so tests do not conflate fallible `Res`/`Opt` behavior with fatal termination.

Required suites do not use `diagnostic_contains`. Optional suites may use it
with `expect.value` for non-blocking rendering probes.

For `compile_error`/`runtime_error`, use field-scoped tokens:

1. `message_contains` (required)
2. `message_contains_any` (required when the machine contract defines a one-of group)
3. `data_keys` (required; use `[]` when no diagnostic `data` keys are asserted)
4. `data_equals` (optional exact-match map for contract-authorized keys)

`compile_error` may additionally assert a marker-resolved source line:

```yaml
code: |
  let first = move root
  let second = move root // @primary second-move
expect:
  compile_error:
    code: OWNERSHIP_DOUBLE_MOVE
    primary_span:
      marker: second-move
```

The `// @primary <name>` comment must end its source line. Names use lowercase
ASCII kebab case and are unique across the fixture's `code` or `.eglur` entries
in `files`.
The runner compares the marker's source unit and line with the diagnostic
source span and its text start. Marker assertions do not constrain bytes,
columns, or end positions. Primary markers are compile-error-only and
unavailable to `code_bytes_hex`.

Any source form may instead assert an exact physical byte range:

```yaml
code_bytes_hex: ff
expect:
  compile_error:
    code: LEX_INVALID_UTF8
    primary_span:
      source: code
      bytes: {start: 0, end: 1}
```

`source` names one fixture source unit, byte offsets are zero-based, and `end`
is exclusive. The range MUST fit that source's exact physical bytes.

`docs/spec/errors/diagnostic-contracts.yaml` is the normative source for these
fields. In required suites:

1. `message_contains` contains all portable all-of literals and one concrete
   rendering for each message template; it may be `[]`.
2. `message_contains_any` exactly mirrors contract one-of groups.
3. `data_keys` contains unconditional and active conditional keys only.
4. `data_equals` keys must appear in both `data_keys` and the contract's
   `exact_value_keys`.
5. `help_contains`, `related_contains`, and `diagnostic_contains` are forbidden.

For `expect.value`:

1. keep `kind` required;
2. use `repr` for textual/non-numeric snapshot checks;
3. use `numeric_compare` for numeric/tensor semantic checks:
   - `dtype`, `expected` required,
   - `shape` required for tensor values,
   - `mode` defaults to `approx` for floating/complex and `exact` otherwise,
   - `rtol`/`atol` optional (default from spec conventions).
4. for code-level value-channel diagnostics, use `value_diagnostics` entries with:
   - `code` (required),
   - `message_contains` (required),
   - `message_contains_any` (when required by the machine contract),
   - `data_keys` (required; may be `[]`),
   - `data_equals` (optional, contract-authorized exact `Str | i64 | Bool` map).
5. use `kind: Unit` with `process_exit: 0` to observe a `Unit` entrypoint result
   without inventing a textual snapshot.

For `expect.test_run`:

1. use `order` for the complete reported result order, matching the discovery order key in `CONFORMANCE.md`;
2. use `passed`, `skipped`, and `failed` to partition all entries in `order`;
3. identify tests by structured `{module, name}` pairs rather than relying on implementation-specific report rendering;
4. each `failed` entry requires:
   - `kind: returned_err` for `Res[Unit, E]` `Err(e)` failures,
   - `kind: runtime_abort` plus `runtime_code` for fatal abort failures,
   - `message_contains` tokens for deterministic failure evidence.

## Test ID Policy

Test IDs are stable suite-local identifiers used by `traceability.yaml` and related
release/sign-off tooling.

Policy:

1. The canonical form is `PREFIX-semantic-slug`, where `PREFIX` is the suite family
   tag and `semantic-slug` is a normalized lowercase ASCII slug derived from `title`.
2. Slug normalization MUST:
   1. lowercase ASCII letters,
   2. collapse non-alphanumeric runs to single `-`,
   3. trim leading/trailing `-`,
   4. produce a non-empty slug.
3. IDs MUST be globally unique across the conformance tree.
4. If deterministic title-derived slugs collide, disambiguate with numeric suffixes:
   `...-2`, `...-3`, and so on.
5. `tests/conformance/tools/rekey_ids.py` is the canonical helper for title-derived
   semantic ID migration and rewrite preview.

Preview/apply semantic ID rewrites with:

```bash
pixi -q run --locked rekey [--write] [--rewrite-refs]
```

With no path arguments, the tool rekeys exactly the suite files listed by
`manifest.yaml`; implementation-probe YAML is not a suite. `--rewrite-refs`
still updates exact test-ID references in the registry-cache profile.

## Suite Membership

`manifest.yaml` is the only maintained suite inventory. Add or move a suite
there rather than copying file lists into documentation. Its `required` and
`precondition` fields determine baseline selection under the normative
[required-versus-optional
rules](../../docs/spec/CONFORMANCE.md#required-vs-optional-suites).

`probes/registry_cache.yaml` is an implementation probe discovered through the
artifact inventory, not a manifest suite. Optional extension and
emit-if-present diagnostic probes remain manifest entries with
`required: false`; their release treatment is defined by the
[optional emit-if-present
policy](../../docs/spec/RELEASE-GATE-v0.1.md#optional-emit-if-present-policy).

## Coverage and Validation

### Diagnostic Traceability

`traceability.yaml` is the machine-checkable mapping from required diagnostic
codes (`docs/spec/errors/compile-codes.md`, `docs/spec/errors/extension-codes.md`, `docs/spec/errors/runtime-codes.md`)
to concrete conformance tests.

Coverage strength:

1. `asserted`: code is asserted directly via `expect.compile_error.code`, `expect.runtime_error.code`, or structured `expect.value_diagnostics[*].code`.
2. `mentioned_only`: code appears only as a token mention (for example in a
   title or optional-suite rendering probe) and is weaker coverage.
3. `uncovered`: no matching evidence in required suites.

Policy levels in `traceability.yaml`:

1. `hard_required_asserted`: baseline required and MUST be `asserted`.
2. `conditional_required`: required only when listed precondition is active.
   - precondition keys used by `conditional_required` entries:
     - `strict_unavailable_path` (for `DETERMINISM_UNAVAILABLE`, value-channel assertion),
     - `ffi_linkage_probe` (for `EXTERN_LINKAGE_FAILED`).
3. `optional_emit_if_present`: note/warning-like diagnostics; emission optional, but if emitted token/data contract is required.
4. `excluded_nonbaseline`: extension/non-baseline path excluded from baseline gate.

Change suites or diagnostic catalogs first, then regenerate the derived
traceability artifact:

```bash
pixi -q run --locked generate-traceability
pixi -q run --locked validate-traceability
```

### Named Pixi Tasks

Invoke repository tooling through the named tasks in `pixi.toml` so each command
uses the locked environment.

Suite-author tasks:

| Task | Purpose |
| --- | --- |
| `rekey` | Preview or apply semantic test-ID rewrites; accepts `--write` and `--rewrite-refs` |
| `validate-schema` | Validate manifest, suite, and registry-cache schemas |
| `generate-traceability` | Regenerate `traceability.yaml` from suites and diagnostic catalogs |
| `validate-traceability` | Check that generated traceability is current |
| `test` | Run conformance-tooling regression tests |

Repository-wide maintenance and validation tasks:

| Task | Purpose |
| --- | --- |
| `validate-artifacts` | Validate the authoritative artifact inventory and checkout binding |
| `validate-open-items` | Validate OPEN sources against the release ledger |
| `validate-effects` | Validate standard-library effect summaries |
| `validate-doc-links` | Validate local targets and fragments in tracked Markdown |
| `render-effects` | Render the informative effect-summary reference |
| `validate-keywords` | Validate normative keyword usage |
| `check` | Run lint, format, type, test, and static validation gates |

Use the standard invocation form:

```bash
pixi -q run --locked <task>
```

The `test` task must discover at least one test; zero-test discovery is a
failure. Release-gate invocations and their verdict boundaries are documented
in the [release and evidence guide](RELEASE.md).
