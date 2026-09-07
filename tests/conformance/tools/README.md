# Conformance Tooling Architecture

This guide is for maintainers of the Python tooling under this directory.
Suite authors should use the [suite author guide](../README.md), and release
operators should use the [release and evidence guide](../RELEASE.md).

## Supported Interfaces

Named Pixi tasks are the supported entry points for contributors and
integrations. They invoke the command scripts directly under
`tests/conformance/tools/`. Keep those adapters' paths, flags, output, and exit
meanings stable.
Reusable semantics belong to the `eglur_conformance` package. Tests import the
package owner for semantic behavior and execute scripts for CLI behavior; they
do not import sibling scripts or modify `sys.path`.

The supported environment and task surface are defined by repository-root
`pixi.toml` and `pixi.lock`. Bootstrap it with `pixi install --locked`, then run
the complete Python tooling gate with:

```bash
pixi -q run --locked check
```

## Documentation Link Boundary

`validate-doc-links` reads every Git-tracked `.md` file. It checks single-line
inline and image links plus reference-style local destinations against exact
tracked worktree paths, and validates fragments against ATX heading anchors.
Tracked files and directories derived from tracked files may be targets.
`--repo-root` must name the exact Git worktree root; a repository subdirectory
is rejected.

Fenced and indented code, inline code spans, and HTML comments are not link
sources. URI schemes and protocol-relative URLs are treated as external and
are not fetched. Raw HTML `href` and `src` attributes are outside this command's
Markdown-link boundary.

## Semantic Owners

1. `release_identity.py` owns release labels and history-root validation.
   `artifact_content.py` owns canonical JSON, SHA-256 addressing, immutable byte
   snapshots, and primitive artifact-field narrowing. `validation.py` owns the
   shared validation-problem value.
2. `yaml_document.py` and `markdown_document.py` provide shared boundary parsing
   without owning domain policy.
3. `diagnostics/` owns diagnostic contracts, catalog composition, profiles, and
   coverage policy. `effects/` owns effect-catalog extraction and rendering.
4. `suite/` owns manifest and test-case models, strict decoding, schema
   validation, semantic ID rekeying, and the serialized conformance-bundle
   snapshot and address.
5. `frontend/` owns the generated parser-input corpus, focused syntax cases,
   public-example bindings, and exact freshness checks.
6. `semantic_slice/` owns the identity-only first-slice selection and its
   generated source, invocation, and language-expectation projection. Compiler
   capabilities, invariant probes, and observed validation evidence remain
   outside this package.
7. `registry_cache/` owns the cache-probe model, strict codec, source-fixture
   binding policy, and repository validation.
8. `traceability/` owns diagnostic-to-test analysis, the canonical report, its
   codec, and repository projection.
9. `documentation/` owns tracked Markdown link validation. `model.py` carries
   canonical observations, `syntax.py` projects visible Markdown,
   `links.py` applies repository-local resolution policy, and `repository.py`
   owns the Git/worktree boundary.
10. `evidence/` owns execution contexts and compile- or execution-phase evidence.
   Each phase package separates canonical models, artifact codecs, suite-derived
   projections, and cross-artifact policy. `execution_context/` contains only
   its model and codec.
11. `spec_release/` is the outer composition layer. It owns artifact inventory,
   OPEN-item state, release history, sign-off data, Git access, and release-gate
   policy while consuming the lower domain packages.

## Dependency Direction

The intended flow is:

```text
CLI/repository boundary -> document codec -> canonical model
                        -> domain analysis or policy -> artifact/verdict
```

Boundary modules resolve paths and read files or Git objects. Codecs normalize
input once; analysis and policy operate on canonical values. Traceability and
evidence consume suite and diagnostic facts. `spec_release.gate` retains the
inactive standalone release composition for future process design; the
supported CLI reports `UNAVAILABLE`. Use `pixi run check` for development
validation, not the retained history or sign-off components. See
[release operations](../RELEASE.md).

Evidence callers import the relevant phase's `model`, `codec`, `projection`, or
`policy` module directly. Package initializers do not provide facade aliases.
Stable primitives sit below domain packages. No semantic-owner package below
`spec_release` imports it. The semantic-owner graph remains acyclic.
`test_package_topology.py` enforces these rules.

## Test Ownership

Package tests cover semantic behavior through the owning module. CLI tests
execute the top-level adapter and assert flags, output scope, and exit status.
`test_package_topology.py` covers dependency direction and evidence-role
boundaries within the Python package.
`test_compiler_package_topology.py` separately checks the Rust workspace member
set, local dependency targets, and production crate graph. The complete
`check` task runs Ruff, Pyrefly, unit tests, and static specification validators.
