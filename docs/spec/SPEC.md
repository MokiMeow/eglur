# Eglur Language Specification

Status: Normative v0.1.0 draft.

Scope: Linked sections are normative for v0.1 unless they state otherwise.

## Section Index

1. [Document Status](language/status.md)
2. [Conformance Keywords](language/keywords.md)
3. [Package and Dependency Model](language/package.md)
4. [Scope and Identity](language/scope.md)
5. [Core Invariants](language/invariants.md)
6. [General Core and Tensor Layer Boundary](language/implementation-boundary.md)
7. [Program and Evaluation Model](language/program-model.md)
8. [Syntax and Grammar](language/syntax.md)
9. [Modules and Name Resolution](language/modules-resolution.md)
10. [Type System Baseline](language/type-system.md)
11. [Algebraic Data Types (`enum`)](language/algebraic-data.md)
12. [Bindings and Struct Semantics](language/bindings-structs.md)
13. [Calls and Reference Semantics](language/calls-references.md)
14. [Ownership Boundary](language/ownership-boundary.md)
15. [Control Flow](language/control-flow.md)
16. [Compile-time vs Runtime Validation Matrix](language/validation-matrix.md)
17. [Effect Semantics](language/effects.md)
18. [Failure and Optionality](language/failure.md)
19. [Sequence, Traverse, and Sequence Conversion](language/seq-traverse.md)
20. [Traits and Dispatch](language/traits.md)
21. [Concurrency and Parallelism](language/concurrency.md)
22. [Tensor Transform Model](language/tensor-model.md)
23. [Tensor Axis Overlay Semantics](axes.md)
24. [Unsafe and FFI Boundary](language/unsafe-ffi.md)
25. [Compile-time Complexity Constraints](language/compile-complexity.md)
26. [Conformance Mapping](language/conformance.md)

## Release Control and Future Work

[Language open items](open-items/language.md) are authoritative release-control
records, but they do not define v0.1 language semantics. Future design records
live in the [roadmap](../roadmap/README.md), and specification authoring guidance
lives under [contributor documentation](../contributing/spec-style.md).
