# Contributing to Eglur

This repository maintains the Eglur specification package, executable
conformance definitions, and their supporting tools. Change each contract in
its authoritative document, then update the affected guides and tests.

## Repository Areas

| Area | Responsibility |
| --- | --- |
| [`docs/spec/`](docs/spec/) | Normative language, standard-library, diagnostic, conformance, and release contracts |
| [`docs/spec/open-items/`](docs/spec/open-items/) | Canonical detail for unresolved or explicitly deferred decisions |
| [`docs/roadmap/`](docs/roadmap/) | Selected post-v0.1 directions that are outside the current baseline |
| [`tests/conformance/`](tests/conformance/) | Canonical suite manifest, executable fixture definitions, evidence schemas, and conformance tools |
| [`tests/frontend/`](tests/frontend/) | Authored focused syntax cases and the generated production front-end corpus |
| [`compiler/`](compiler/) | Rust compiler crates for source handling, parsing, HIR, diagnostics, semantic analysis, and bounded evaluation |
| [`docs/contributing/`](docs/contributing/) | Contributor-specific writing and maintenance guidance |

For normative prose, follow the [spec writing
guide](docs/contributing/spec-style.md). The [artifact
inventory](docs/spec/artifact-inventory-v0.1.yaml) defines which files belong
to a spec-package release and which role each file serves.

## Tooling Environment

The locked environment is defined by `pixi.toml` and `pixi.lock`. It supports
`osx-arm64` and `linux-64`, requires Pixi 0.76.x, and selects Python 3.11. The
repository does not install `eglur_conformance` as a public Python package.
Compiler work additionally requires a current stable Rust toolchain with Cargo,
rustfmt, and Clippy. Rust is not installed or version-pinned by Pixi; the root
`Cargo.lock` fixes Rust crate dependencies.

Install the environment and run the complete Python tooling and specification
validation gate with:

```bash
pixi install --locked
pixi -q run --locked check
```

Named Pixi tasks are the supported contributor and release interface. The
scripts under `tests/conformance/tools/` are their canonical executable
adapters; direct script invocation is not a separate supported interface.
See the [suite author guide](tests/conformance/README.md) for focused validation
tasks, the [tooling architecture guide](tests/conformance/tools/README.md) for
package maintenance, and the [release operations
guide](tests/conformance/RELEASE.md) for release commands. The [compiler
guide](compiler/README.md) defines the Rust workspace, dependency direction,
parser failure boundary, measurement contract, and focused compiler tasks.

## Change Discipline

- Update each contract at its canonical owner and reconcile affected indexes,
  diagnostics, fixtures, traceability data, and derived documentation.
- Keep current v0.1 requirements in `docs/spec/`, unresolved decisions in
  `docs/spec/open-items/`, and post-v0.1 designs in `docs/roadmap/`.
- Treat conformance YAML as executable contract data rather than explanatory
  prose.
- Run focused checks while iterating. Use `pixi -q run --locked check` for
  specification and conformance-tooling changes. Production compiler changes
  use the Cargo gate in the [compiler guide](compiler/README.md).
