# Eglur

Eglur is a compiled language with explicit effects, recoverable failure,
visible ownership handoffs, left-to-right composition, and first-class tensor
support. This repository contains the language specification, conformance
definitions, and the tooling used to maintain them. A [Rust compiler
workspace](compiler/README.md) implements the lexer/parser and a bounded
semantic and execution subset; it is not a complete v0.1 implementation.

The specification is a working draft targeting Eglur 0.1.0. It evolves with
the implementation; there is no separate specification release. Pin a commit
when referring to a particular draft. The eventual Eglur release will identify
its specification revision and supported implementation scope.

## Start Here

| Reader | Start with | Continue with |
| --- | --- | --- |
| Language evaluator or user | [Language overview](OVERVIEW.md) | [Language specification](docs/spec/SPEC.md) and [standard library](docs/spec/STDLIB.md) |
| Language implementer | [Language specification](docs/spec/SPEC.md) | [Diagnostics](docs/spec/ERRORS.md), [conformance](docs/spec/CONFORMANCE.md), and the [implementation boundary](docs/spec/language/implementation-boundary.md) |
| Conformance author | [Conformance author guide](tests/conformance/README.md) | [Suite manifest](tests/conformance/manifest.yaml) and [conformance rules](docs/spec/CONFORMANCE.md) |
| Tooling maintainer | [Tooling architecture](tests/conformance/tools/README.md) | [Contributor guide](CONTRIBUTING.md) and [compiler guide](compiler/README.md) |
| Release operator | [Release policy and availability](tests/conformance/RELEASE.md) | [Evidence contracts](docs/spec/RELEASE-GATE-v0.1.md) and [artifact inventory](docs/spec/artifact-inventory-v0.1.yaml) |
| Design maintainer | [Open-item ledger](docs/spec/open-items-ledger-v0.1.yaml) | [Roadmap](docs/roadmap/README.md) and [spec writing guide](docs/contributing/spec-style.md) |

## Specification Set

The normative v0.1 documents are:

- [Language specification](docs/spec/SPEC.md)
- [Standard library](docs/spec/STDLIB.md)
- [Tensor axis semantics](docs/spec/axes.md)
- [Diagnostics](docs/spec/ERRORS.md)
- [Conformance rules](docs/spec/CONFORMANCE.md)
- [Release availability and evidence contracts](docs/spec/RELEASE-GATE-v0.1.md)

The [artifact inventory](docs/spec/artifact-inventory-v0.1.yaml) identifies the
complete release set and the role of each artifact. The [suite
manifest](tests/conformance/manifest.yaml) is the authoritative conformance
suite inventory.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the repository layout, locked tooling
environment and quality gates.

## License

Copyright 2026 Eglur contributors.

Except where otherwise noted, the contents of this repository, including the
Eglur specification, conformance definitions, and repository tooling, are
licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).

An independently developed Eglur compiler, runtime, or other implementation
does not inherit this license merely by implementing the specification.
