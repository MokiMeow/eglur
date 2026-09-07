# Program and Evaluation Model

Scope: Normative for v0.1 unless this file or a section states otherwise.

This file is the normative entry point for core language execution semantics.

Normative content is split into focused modules:

1. [Syntax and Grammar](syntax.md)
2. [Modules and Name Resolution](modules-resolution.md)
3. [Type System Baseline](type-system.md)
4. [Algebraic Data Types (`enum`)](algebraic-data.md)
5. [Bindings and Struct Semantics](bindings-structs.md)
6. [Calls and Reference Semantics](calls-references.md)
7. [Ownership Boundary](ownership-boundary.md)
8. [Control Flow](control-flow.md)
9. [Compile-time vs Runtime Validation Matrix](validation-matrix.md)
10. [Effect Semantics](effects.md)
11. [Failure and Optionality](failure.md)
12. [Unsafe and FFI Boundary](unsafe-ffi.md)

Conformance and diagnostic mapping references to `program-model.md` remain valid and refer to the aggregate of the linked sections above.

## Executable Invocation and Entrypoint

An executable invocation MUST designate exactly one entry source:

1. For a single-source invocation, the sole source is the entry source.
2. For a file-set invocation, the request's `entry` path identifies the entry
   source under `modules-resolution.md`.
3. For a package invocation, the request's `entry` path MUST identify a
   validated `.eglur` source under the package `src/` root.

Only top-level declarations in the entry source are entrypoint candidates.
A `main!` declaration in another source is an ordinary module member and does
not compete with the selected entrypoint.

An executable program MUST contain exactly one top-level `main!` declaration in
the entry source. The declaration MUST:

1. have no parameters,
2. have no generic parameters, and
3. have a declared return type that, after transparent alias expansion, is
   either:
   1. `Unit`, or
   2. `Res[T, E]`, where `T` and `E` are ownership-free and reference-free, or
   3. `Never`.

`main` without the bang marker is an ordinary function and is not an
entrypoint. Entrypoint execution is effectful even when the selected body
performs no externally visible effect.

Entrypoint diagnostics are:

1. no top-level `main!` in the entry source:
   `PROGRAM_ENTRY_MISSING`,
2. more than one top-level `main!` in the entry source:
   `PROGRAM_ENTRY_DUPLICATE`,
3. exactly one candidate whose parameter, generic-parameter, or normalized
   return-type shape violates this section:
   `PROGRAM_ENTRY_SIGNATURE_INVALID`.

Entrypoint validation runs only for executable invocations. Library/build-only
compilation and the conformance `runner: test` path do not require `main!`.

Invocation/package validation and ordinary source compilation diagnostics take
precedence over entrypoint diagnostics. Implementations MUST defer ordinary
duplicate/overload diagnostics for top-level `main!` candidates in the entry
source so that, if no other compile failure exists,
`PROGRAM_ENTRY_DUPLICATE` is the stable failure for multiple candidates.

For source-backed diagnostics:

1. `PROGRAM_ENTRY_DUPLICATE` uses the second candidate in source order as its
   primary source span.
2. `PROGRAM_ENTRY_SIGNATURE_INVALID` uses the invalid `main!` declaration as
   its primary source span.
3. `PROGRAM_ENTRY_MISSING` has no declaration-owned span. It uses the
   zero-width physical byte range `0..0` in the designated entry source, with a
   text projection beginning and ending at line 1, column 1.

## Process Result Projection

The executable runner MUST first observe the selected `main!` return value, then
project it to a process exit status:

1. `()` of type `Unit` returns process exit status `0`.
2. `Ok(_)` returns process exit status `0`.
3. `Err(_)` returns process exit status `1`.
4. An unrecoverable runtime abort returns process exit status `1`.
5. A `main!` declared as `Never` has no normal return value and therefore no
   normal process-result projection. It may continue indefinitely or terminate
   through an existing runtime-fatal path; the latter follows Rule 4.

This projection MUST NOT render the returned value or emit implicit stdout or
stderr output. In particular, `T` and `E` do not require `Display`. A compile
failure produces no executable process and therefore has no process exit
status.
