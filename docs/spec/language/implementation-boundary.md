# Language, General Stdlib, and Tensor Boundary

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Dependency Direction

The normative semantic layering direction is:

```text
language core -> required general stdlib/platform -> bounded tensor source layer -> Core Tensor IR/AD -> kernel IR/backend/runtime
```

Each layer to the right can consume contracts owned by layers to its left; a
layer to the left MUST NOT depend on semantics owned only by a layer to its
right. The arrow also follows validation and lowering flow. It does not
prescribe a source-tree layout or module-import graph.

Implementations MAY share lexer, parser, type-representation, diagnostic, and
optimization infrastructure across layers, but shared infrastructure MUST NOT
introduce a reverse semantic dependency.

The layers are:

1. The language core owns modules, declarations, ordinary expressions
   and calls, nominal types, ADTs, traits and coherence, effects, failure,
   ownership, control flow, and intrinsic call-composition syntax.
2. The required general standard-library and platform layer owns scalar and
   collection APIs, strings, I/O, time, networking, concurrency, and catalog
   pipeline adapters. It is required v0.1 functionality, but it is not
   language-core syntax or type-system semantics.
3. The bounded tensor source layer owns the closed tensor-specific source
   forms, tensor catalog identities, schema normalization, axis and shape
   constraints, and tensor-specific validation.
4. Core Tensor IR and AD own normalized pure tensor graphs, tensor lowering,
   and program transforms over those graphs.
5. Kernel IR, backends, and runtime own execution planning, code generation,
   device integration, and execution downstream of Core Tensor IR.

Language-core and required general-stdlib semantics MUST NOT depend on tensor
axis, shape, dtype, device, schema, lowering, or backend facts. The tensor
source layer MAY reuse language-core name resolution, nominal typing, traits,
effects, `Res`/`Opt`, ownership, and call semantics, together with required
general-stdlib value and catalog contracts, without redefining them.

### Closed Tensor Source Activation Surface

The v0.1 tensor source layer is activated only by at least one of:

1. an `AxisNameLit` or `AxisIndexLit`,
2. a `TensorSchemaExpr` (`view`, `rearrange`, `inflate`, `reduce`, `contract`,
   or `einop`),
3. a `DTypeBracketCallSuffix` whose callee resolves to an eligible canonical
   `std::tensor` constructor,
4. a `ReduceByCandidateSuffix` whose receiver has canonical `ReduceSchema`
   identity,
5. ordinary name resolution of an explicitly referenced `std::tensor`
   declaration, including the prelude re-export of `ReadableTensor`.

Merely lexing a same-spelled identifier such as `view`, making
`ReadableTensor` available in the prelude, or hosting tensor grammar
productions in a shared parser does not activate tensor schema solving,
lowering, or runtime support.

A compilation containing none of the activation forms above is a non-tensor
compilation. Such a compilation:

1. MUST type-check without invoking tensor axis, shape, or dimension solving,
2. MUST NOT construct a tensor schema semantic model or Core Tensor IR,
3. MUST NOT require tensor runtime initialization, device discovery, kernel
   planning, or a tensor backend,
4. MUST produce the same ordinary name-resolution, inference, effect, failure,
   ownership, and control-flow results whether tensor support is present in the
   implementation or not.

Loading declaration metadata needed to expose the prelude's canonical
`ReadableTensor` symbol does not violate these rules. It MUST NOT trigger any
tensor runtime or backend behavior.

### Surface Ownership

| Facility | Normative owner | Boundary rule |
| --- | --- | --- |
| `TensorSchemaExpr` and transform-axis grammar | tensor language syntax | Closed compiler-known forms; not an open parser extension |
| `AxisNameLit`, `AxisIndexLit`, and axis-pack tokens | tensor language syntax | Language-owned token forms with tensor-layer semantics |
| dedicated tensor constructor dtype brackets | tensor language syntax plus canonical catalog identity | Ordinary bracket indexing remains absent |
| `ReduceByCandidateSuffix` phase grammar | tensor language syntax plus canonical `ReduceSchema` identity | Other receivers retain ordinary member-call semantics |
| `AxisName`, `AxisIndex`, `Tensor`, `TensorView`, `DType`, schema values, capabilities, and tensor errors | `std::tensor` catalog and semantics | Ordinary nominal types and declarations at the general type-checker boundary |
| axis-expression normalization, dimension solving, pack partitioning, shape checks, and tensor validation order | tensor constraint system | Cannot participate in ordinary trait solving or general inference |
| scalar/tensor numeric promotion | required general standard-library conventions | Tensor typing consumes the shared lattice; general scalar typing does not depend on tensor types |
| Core Tensor IR primitive graph and AD lowering | Core Tensor IR | Constructed only after tensor source and constraint validation succeeds |
| kernel uops, fusion, scheduling, code generation, devices, and backend execution | kernel IR/backend/runtime | MUST NOT change source typing or validation classification |

### Frontend and Lowering Obligations

1. A shared lexer and parser MAY recognize the closed tensor token and grammar
   forms, but MUST hand their semantic interpretation to the tensor language
   layer.
2. Ordinary identifiers and calls MUST complete normal lookup and typing
   without consulting tensor schema or backend state.
3. The general type checker owns nominal constructor arity, ordinary callable
   compatibility, generic substitution, trait obligations, effects, and result
   types. The tensor constraint system owns only the constraints listed in the
   table above.
4. Tensor schema parsing MUST normalize to one bounded compiler-known schema
   model before dimension, pack, shape, or lowering work begins.
5. Core Tensor IR MUST be constructed only after source typing and all
   applicable tensor-constraint checks have succeeded.
6. Backend availability, selected kernels, and optimization plans MUST NOT
   influence name resolution, type inference, or frontend/source-validation
   diagnostic selection. Runtime capability checks MAY produce only the
   standardized runtime/value-channel outcomes assigned to them, including
   `DETERMINISM_UNAVAILABLE` for an explicit unsupported strict request.
7. Every output-relevant backend selection made after Core Tensor IR lowering
   MUST be frozen into a reportable execution plan identity before observed
   value evaluation. Repeated evaluation under one fixed identity is governed
   by the bitwise-repeatability contract in `../stdlib/conventions.md`.

### Extension Boundary

The tensor activation list is closed in v0.1. Adding another compiler-known DSL,
schema literal, sigil, parser extension, or macro mechanism requires a later
specification decision. The required tensor source layer is not an optional
conformance profile, and its presence does not make tensor runtime or backend
support a dependency of non-tensor compilations.
