# Release and Evidence Operations

This guide documents supported release-tool invocations. It does not redefine
release readiness or implementation conformance. Those contracts belong to the
[normative release gate](../../docs/spec/RELEASE-GATE-v0.1.md).

## Authority

The release validator composes these canonical inputs:

- [artifact inventory](../../docs/spec/artifact-inventory-v0.1.yaml),
- [suite manifest](manifest.yaml),
- [generated diagnostic traceability](traceability.yaml),
- [OPEN-item ledger](../../docs/spec/open-items-ledger-v0.1.yaml), and
- the operator-supplied spec-package sign-off.

The normative document defines their schemas, identity binding, freeze
conditions, and verdict semantics.

## Spec-Package Readiness

For a new repository, set `release_history_root: repository-root` in all four
release-state artifacts before creating the initial commit. That commit can
contain a `draft` or `released` snapshot. The validator resolves the origin
from Git; do not try to put the initial commit's own hash in its files.
Create the sign-off outside the checkout after the candidate commit exists,
using that commit's exact ID. Subsequent commits follow the ordinary release
transitions. A repository adopting the release policy after earlier work may
instead name an existing, release-state-free ancestor as its fixed boundary.

Run the active gate with an explicit sign-off:

```bash
pixi -q run --locked release-gate spec-package \
  --signoff path/to/spec-release-signoff.yaml
```

`spec-package` also accepts `--repo-root <path>`; relative sign-off paths are
resolved from that root. Exit `0` reports
`spec-package release readiness: GO`. After successful argument parsing,
invalid inputs or unmet conditions report `NO-GO` and exit `1`; argument errors
use the parser's exit `2`.

A `GO` confirms that the versioned specification package is ready for release;
the command does not publish it. It is not an implementation-conformance
verdict or a language/toolchain release. The
[sign-off format](../../docs/spec/RELEASE-GATE-v0.1.md#spec-package-sign-off-format)
and [CLI/CI boundary](../../docs/spec/RELEASE-GATE-v0.1.md#cli-and-ci-boundary)
are normative.

## Implementation Conformance

```bash
pixi -q run --locked release-gate implementation
```

The v0.1 repository has no released executable-evidence profile. This command
therefore reports `implementation conformance: UNAVAILABLE` and exits `2`.
It accepts no evidence flags and cannot produce `PASS`. The conditions for
activating implementation certification remain in [Current Certification
Availability](../../docs/spec/RELEASE-GATE-v0.1.md#current-certification-availability)
and [Implementation Profile Activation
Requirements](../../docs/spec/RELEASE-GATE-v0.1.md#implementation-profile-activation-requirements).

## Evidence Boundary

The tooling package contains codecs and policy checks for context-addressed
execution evidence and compile-phase observations. These components validate
artifact shapes and bindings; they do not run a compiler, authenticate a
physical execution, or activate certification. The complete contracts are:

- [Context-Addressed Execution Evidence](../../docs/spec/RELEASE-GATE-v0.1.md#context-addressed-execution-evidence)
- [Compile-Phase Observation Evidence](../../docs/spec/RELEASE-GATE-v0.1.md#compile-phase-observation-evidence)
- [Optional Emit-if-present Policy](../../docs/spec/RELEASE-GATE-v0.1.md#optional-emit-if-present-policy)

There is no separate supported evidence-certification command while
`implementation` remains unavailable.
