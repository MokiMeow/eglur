# Release and Evidence Operations

## Current Policy

The specification is a working draft targeting Eglur 0.1.0. There is no
separate specification release. Pin a Git commit when referencing a draft;
the public repository is not itself a release or an implementation certificate.

An eventual Eglur release will identify both the specification revision and
implementation support. Its acceptance criteria, packaging, and publication
procedure remain to be established. The [release policy](../../docs/spec/RELEASE-GATE-v0.1.md#release-policy)
records this boundary.

## Development Checks

```bash
pixi -q run --locked check
```

This checks the Python tooling, artifact inventory, suite schemas, effect
catalog, document links, traceability, OPEN items, and generated frontend and
semantic expectations. Compiler checks are documented in the
[compiler guide](../../compiler/README.md). None of these checks alone certifies
a complete implementation.

The inventory, manifest, traceability, and OPEN-item ledger carry the same
`release: v0.1.0` target and `status: draft`. Keep generated evidence current
when their inputs change. Draft edits do not require a version bump merely
because an earlier commit carried a `released` marker.

## Unavailable Release Operations

```bash
pixi -q run --locked release-gate spec-package
pixi -q run --locked release-gate implementation
```

Both commands report their scoped `UNAVAILABLE` status on stderr and exit `2`.
Neither accepts evidence arguments, publishes anything, or issues `GO` or
`PASS`. They do not require a Git checkout or sign-off file to report their
availability.

## Retained Evidence Design

The tooling contains codecs, static checks, and policy components for
context-addressed execution evidence, compile-phase observations, and the
inactive standalone spec-package release design. They are not supported
release or certification entry points.

The [evidence contracts](../../docs/spec/RELEASE-GATE-v0.1.md) retain those
schemas and the previous standalone sign-off and history design for review
when maintainers define the integrated release process. In particular, the
standalone history transition validator is not a development gate. Do not
apply its freeze rules to the current draft or treat a component-level result
as release approval.
