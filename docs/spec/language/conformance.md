# Conformance Mapping

Scope: Normative for v0.1 unless this file or a section states otherwise.

An implementation is conformant to the language specification indexed by `../SPEC.md` only if:

1. It satisfies all normative `MUST` and `SHALL` statements in the indexed language specification set.
2. It passes required suites defined in `../CONFORMANCE.md`.
3. It emits required diagnostics as defined in `../ERRORS.md`.
4. It preserves the compile-time vs runtime boundary defined in `program-model.md`, `tensor-model.md`, and `validation-matrix.md`.

Suite ownership follows `implementation-boundary.md`:

1. Core lexer and parser suites MAY cover integrated recognition of axis tokens,
   contextual schema literals, and their disambiguation from ordinary
   identifiers.
2. Tensor suites own axis-literal type identity, schema semantics, tensor
   constraints, lowering-visible tensor behavior, and tensor diagnostics.
3. Core suites MUST retain ordinary declarations, calls, inference, and trait
   behavior for tensor-contextual spellings used outside the closed tensor
   forms.
4. The prohibition on invoking tensor solving, Tensor IR construction, or a
   tensor backend for a non-tensor compilation is an implementation
   architecture obligation. Portable source fixtures cannot observe an absent
   internal call; implementations SHOULD additionally enforce it with internal
   unit tests or instrumentation.

Release-control OPEN items do not add mandatory v0.1.0 conformance requirements.
