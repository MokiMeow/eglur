# Scope and Identity

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Scope

Eglur targets compiled programs and pipelines that prioritize predictable
semantics, explicit effects and failures, visible ownership handoffs, and
reviewable composition.

Implementations MUST prioritize predictable semantics and fast compilation.

Eglur has a generally usable language core and required general standard
library, paired with a bounded first-class tensor source layer. The tensor
source layer is required baseline language functionality, not an optional
profile, but language-core and general-standard-library semantics and
non-tensor compilation MUST remain independent of tensor solving, Tensor IR,
and tensor backends under `implementation-boundary.md`. Tensor source forms
expose axis and shape semantics and distinguish zero-copy views, materializing
transforms, and explicit duplication under `tensor-model.md`.


### Non-goals

Eglur is not a systems programming language.

The language core MUST NOT require borrow/lifetime proof systems comparable to Rust.

Local references use the finite lexical storage-containment and provenance
rules in `calls-references.md`. They do not introduce unique borrowing,
last-use regions, lifetime parameters, or interprocedural lifetime inference.

Low-level access MUST be explicit through `unsafe` and FFI boundaries.
