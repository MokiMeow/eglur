# Eglur Standard Library Specification

Status: Normative v0.1.0 draft.

Scope: Linked sections are normative for v0.1 unless they state otherwise.

## Section Index

### Baseline Modules (v0.1)

The modules in this section define the required v0.1 baseline surface.

[`stdlib/effect-summaries.yaml`](stdlib/effect-summaries.yaml) is the normative
machine-readable effect metadata for callable declarations in these baseline
module documents. It does not change whether a declaration is required,
optional, or preview-only within its owning document.

1. [Normative Keywords](stdlib/keywords.md)
2. [Conformance Scope](stdlib/conformance-scope.md)
3. [Global Conventions](stdlib/conventions.md)
4. [Catalog Signature Metalanguage](stdlib/catalog-signatures.md)
5. [Cast Legality and Value Domain](stdlib/cast-matrix.md)
6. [Module Overview](stdlib/modules.md)
7. [`std::prelude` (REQUIRED)](stdlib/prelude.md)
8. [`std::str` (REQUIRED)](stdlib/str.md)
9. [`std::logic` (REQUIRED)](stdlib/logic.md)
10. [`std::opt` (REQUIRED)](stdlib/opt.md)
11. [`std::res` (REQUIRED)](stdlib/res.md)
12. [`std::seq` (REQUIRED)](stdlib/seq.md)
13. [`std::vec` (REQUIRED)](stdlib/vec.md)
14. [`std::map` (REQUIRED)](stdlib/map.md)
15. [`std::set` (REQUIRED)](stdlib/set.md)
16. [`std::parse` (REQUIRED)](stdlib/parse.md)
17. [`std::json` (REQUIRED)](stdlib/json.md)
18. [`std::io` (REQUIRED)](stdlib/io.md)
19. [`std::net` (REQUIRED)](stdlib/net.md)
20. [`std::time` (REQUIRED)](stdlib/time.md)
21. [`std::tensor` (REQUIRED)](stdlib/tensor.md)
22. [`std::concurrency` (REQUIRED)](stdlib/concurrency.md)
23. [`std::test` (OPTIONAL for production, REQUIRED for conformance)](stdlib/test-harness.md)

### Optional-If-Provided Modules

The modules in this section are excluded from required v0.1 conformance. Their
if-provided contracts remain normative.

24. [`std::csv` (OPTIONAL if provided)](stdlib/csv.md)

## Release Control and Future Work

[Standard-library open items](open-items/stdlib.md) are authoritative
release-control records, but they do not define v0.1 library semantics. Candidate
modules and profiles live in the non-normative [roadmap](../roadmap/README.md).
