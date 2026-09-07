# Diagnostic Model

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Contract Authority

`diagnostic-contracts.yaml` is the sole normative authority for the global
`SourceSpan`, `PrimarySpan`, and `RelatedSpan` schemas and for per-code
diagnostic severity, message requirements, and required `data` keys.

1. Per-code Markdown catalogs define triggers, channel rules, and explanatory
   field summaries.
2. The machine contract represents message requirements as stable literals,
   one-of groups, contextual templates, or contextual prose.
3. `help` remains optional, and guidance for its content is non-normative.
4. `related` remains optional to emit. Every emitted item MUST satisfy the
   machine-readable source-span contracts and the rules below.
5. Required conformance fixtures project only portable assertions from the
   machine contract; the spec-package gate rejects fixture constraints that
   exceed that projection.

### Diagnostic Record

A diagnostic record MUST contain at least:

1. `code: Str` (stable identifier, uppercase snake case).
2. `severity: "error" | "warning" | "note"`.
3. `phase: Str` (`lex`, `parse`, `resolve`, `type`, `effect`, `trait`,
   `runtime`, `stdlib`, `axes`, `concurrency`, or an implementation-defined
   extension).
4. `message: Str`.
5. `primary_span: PrimarySpan`.

The list distinguishes stable baseline spellings from implementation-defined
extensions. An implementation MUST NOT report lexer failures as `parse` solely
to avoid `lex`.

Optional fields:

1. `help: Str`.
2. `related: Seq[RelatedSpan]`.
3. `data: Map[Str, Str | i64 | Bool]`.

Diagnostic byte offsets, source coordinates, and `data` numeric payloads
intentionally use `i64` as part of the stable diagnostic interchange model.
These fields are not user-callable size/count/index API boundaries and do not use `Nat` or `PositiveNat`; producers MUST still emit semantically valid values for each field, such as positive source coordinates and zero-based indices.

### Source and Related Spans

`SourceSpan` has this closed shape:

1. `kind: "source"`.
2. `source: Str`, a non-empty source-unit identity.
3. `bytes: {start: i64, end: i64}`, the canonical physical source range.
4. `text: {start: SourceCoordinate, end: SourceCoordinate}`, an optional
   derived logical projection.

Physical byte offsets are zero-based, `end` is exclusive, and both values are
non-negative. `end` MUST NOT precede `start`; equal offsets represent a
zero-width span. The byte range is canonical even when `text` is present.

`SourceCoordinate` contains positive 1-based `line` and `column` values.
`column` counts Unicode scalar values from the start of the logical line. A
text range's `end` is exclusive, MUST NOT precede its `start`, and MAY equal it.
Text coordinates use the source after leading-BOM handling and logical `LF`
normalization from `../language/syntax.md`. A producer MUST omit `text` when
the source is not valid UTF-8 or either byte endpoint is not a Unicode scalar
boundary. When present, `text` MUST project the same physical byte range; it is
not an alternative location.

`PrimarySpan` is exactly one of:

1. a `SourceSpan`, or
2. `{kind: "unavailable"}` when no source-bound blame exists and the
   code-specific rule permits that state.

A source-triggered compile diagnostic MUST use `SourceSpan`. In particular,
invalid UTF-8 uses a byte-only `SourceSpan`, not `unavailable`.

Each item in `related: Seq[RelatedSpan]` has this canonical shape:

1. `span: SourceSpan`.
2. `label: Str` (non-empty).

In JSON-like interchange and plain-text rendering, filesystem-backed `source`
identities use the path's exact `Path.to_str()` value. Harness-owned inline
sources MAY use a stable non-path identity such as `code`.

The `related_span.examples` entries in `diagnostic-contracts.yaml` are
normative shape and ordering examples. They do not require an implementation to
emit `related` for those scenarios.

Example shape:

```json
{
  "code": "EFFECT_VIOLATION",
  "severity": "error",
  "phase": "effect",
  "message": "non-bang function `f` cannot call `print!`",
  "primary_span": {
    "kind": "source",
    "source": "main.eglur",
    "bytes": {"start": 42, "end": 48},
    "text": {
      "start": {"line": 4, "column": 9},
      "end": {"line": 4, "column": 15}
    }
  },
  "help": "mark caller as `f!` or remove effectful call",
  "related": [
    {
      "span": {
        "kind": "source",
        "source": "io.eglur",
        "bytes": {"start": 120, "end": 145},
        "text": {
          "start": {"line": 12, "column": 1},
          "end": {"line": 12, "column": 26}
        }
      },
      "label": "effectful callee declared here"
    }
  ],
  "data": {
    "caller": "f",
    "callee": "print!"
  }
}
```

### Source-Origin Spans Through Desugaring

When a compile-time diagnostic originates from desugared surface syntax, location reporting MUST remain source-oriented.

1. `primary_span` MUST point to the original source span written by the user.
2. Lowered/internal spans MAY be attached only via `related` notes/spans and MUST NOT replace the primary location.
3. Diagnostics MUST remain understandable without reading lowered/internal spans.

### Primary Blame Selection

Code-specific rules in the diagnostic catalogs select the primary source
construct. Unless a code states a stricter rule:

1. A declaration-invalidating diagnostic selects the declaration occurrence
   that makes the declaration invalid.
2. A diagnostic that becomes an error only at an application, access,
   projection, transfer, or use selects that failing source occurrence.
3. Earlier declarations, consumes, and path-merge causes MAY be reported as
   `related` spans, but MUST NOT replace a later failure-bearing use as primary.
4. Selection MUST be independent of compiler phase and traversal order.


### Code Stability

Within a spec draft line (for example v0.2.x):

1. A code identifier MUST keep the same semantic meaning.
2. The message contract for that code MUST remain satisfied.
3. Severity for that code MUST remain stable unless the spec revision explicitly states a breaking change.

`code` identity is cause identity, not channel identity:

1. The same `code` MAY appear in compile or runtime/value channels when `code-channels.md` permits.
2. Channel changes for statically decidable vs runtime-dependent instances MUST NOT rename or fork cause codes.

Cause identity names a diagnostic family, not one root instance. Distinct blame
identities MAY produce separate roots with the same `code`; root identity,
deduplication, and poisoned-fact handling are defined by `ordering.md` and do not
add fields to the diagnostic record.

### Reserved Prefixes

For forward compatibility:

1. `CAPABILITY_` diagnostic-code prefix is reserved for future ownership/capability-split extensions.
2. v0.1 conforming implementations MUST NOT require or emit `CAPABILITY_*` diagnostics unless an extension profile explicitly enables capability features.


### Phase-variant Detection, Code-invariant Reporting

Some causes can be detected in more than one compile phase depending on implementation architecture.

Rules:

1. Detection phase MAY vary (for example `parse` vs `type`) when semantic outcome is equivalent.
2. Diagnostic `phase` field MUST reflect actual detection phase.
3. Stable `code` assignment MUST NOT vary with detection phase.
4. Conformance uses `code` as primary cause identity; `phase` is informational unless a section explicitly constrains it.
