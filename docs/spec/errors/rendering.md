# Rendering Requirements

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Text Rendering (Minimum)

A textual renderer MUST include, on one logical record:

1. `severity`.
2. `code`.
3. `message`.

Recommended format:

```text
error[EFFECT_VIOLATION] non-bang function `f` cannot call `print!`
 --> main.eglur:4:9
= help: mark caller as `f!` or remove effectful call
```

### Primary Location Rendering

Renderers MUST consume the diagnostic's `primary_span` without inventing a
different source location.

1. A source span with `text` renders its `source` and text start coordinate as
   `<source>:<line>:<column>`. A renderer MAY additionally show the exclusive
   text end.
2. A byte-only source span renders
   `<source>:bytes[<start>..<end>)`. It MUST NOT fabricate line or column
   coordinates.
3. An `unavailable` primary span emits no source-location line.
4. For diagnostics caused by desugaring, the source span points to user-written
   surface syntax, not lowered/internal nodes.
5. Lowered/internal spans, when provided, MUST be rendered only as related
   notes/spans.


### Related Span Rendering

When a diagnostic record contains `related`, a plain-text renderer MUST render
every item after the header, any primary location, and optional `help`,
preserving the record sequence from `ordering.md`.

A source span with text coordinates uses:

```text
 = related: <label>
   --> <source>:<line>:<column>-<end-line>:<end-column>
```

A byte-only source span uses:

```text
 = related: <label>
   --> <source>:bytes[<start>..<end>)
```

Filesystem-backed `<source>` values use `Path.to_str()`, and rendered ends
remain exclusive. A renderer MAY omit a text end only for a zero-width span or
when showing a source excerpt makes the covered range unambiguous. It MAY add
styling, but it MUST preserve each label, location, and relative order. It MUST
NOT promote a related span to the diagnostic header or otherwise present it as
the primary failure location.


### Field-scoped Matching Rule

Primary conformance diagnostic checks are field-scoped:

1. `message_contains` matches only `message`.
2. `message_contains_any` contains one or more token groups; each group matches
   when at least one group member occurs in `message`.
3. `data_keys` matches keys in diagnostic `data` map (an empty list asserts no data-key requirements).
4. `data_equals` matches exact `Str`, signed `i64`, or `Bool` values in
   diagnostic `data`.
5. `compile_error.primary_span.marker`, when present, matches the marker's
   source unit and line against the diagnostic source span and its text start.
   It does not constrain bytes, columns, or either end coordinate.
6. `compile_error.primary_span.source` plus `bytes`, when present, matches the
   diagnostic source identity and canonical physical byte range exactly. It
   does not require a text projection unless a code-specific rule does.

All matching is case-sensitive.

Required suites MUST derive `message_contains`, `message_contains_any`,
`data_keys`, and `data_equals` from `diagnostic-contracts.yaml`.
Primary-span marker assertions instead derive from the code-specific
primary-span requirements in `compile-codes.md`.
They MUST NOT assert `help_contains`, `related_contains`, or top-level
`diagnostic_contains`. Optional suites MAY use those fields as non-blocking,
implementation-specific rendering probes.

Implementations MUST satisfy the message and data requirements in
`diagnostic-contracts.yaml`.

### Canonical Structured Type Rendering

Type-valued diagnostic `data` fields use one deterministic, source-like
spelling. This contract applies to `type`, `expected_type`, `actual_type`,
`left_type`, and `right_type`, and to any code-specific field that its catalog
explicitly declares type-valued.

Rules:

1. The renderer consumes the resolved, alias-expanded type fact used by the
   diagnostic. It MUST NOT rerun inference, overload selection, trait solving,
   associated-type search, or dependency resolution.
2. Every projection already normalizable at the diagnostic site is normalized.
   A still-symbolic projection uses its canonical source form such as
   `T::Item`.
3. Intrinsic and prelude types use their canonical names, including `Unit`,
   `Never`, `Bool`, `Str`, `i64`, `Nat`, `Opt`, and `Res`.
   A shadowed primitive name uses `std::prelude::<name>` to distinguish the
   canonical primitive from the visible binding. Import aliases do not replace
   canonical primitive spellings.
4. A named non-prelude declaration uses the shortest source-writable path that
   resolves to that declaration at the diagnostic site. Ties are resolved by
   the canonical lexical text order in `../language/invariants.md`. A
   transparent type-alias name is not a candidate path because the input type
   has already been expanded.
5. Composite types use these exact separators and omit redundant grouping:
   `Name[T, U]`, `(T,)`, `(T, U)`, `Fn(T, U) -> R`, `&T`, `&mut T`,
   `*const T`, and `*mut T`.
6. A normalized multi-member error union is rendered as `Union[A, B, C]`, with
   members sorted by their canonical rendered spelling under canonical lexical
   text order. A singleton is rendered as its sole member.
7. The result is a display contract scoped to one compilation and diagnostic
   site. It MUST NOT contain `PackageInstanceId`, a host filesystem path, or
   another non-portable identity, and consumers MUST NOT treat the string as a
   cross-build nominal type identifier.

A trait-valued field such as `trait` uses the same declaration-path and type-
argument rules. For example, a generic obligation is rendered as
`module::Rel[i64]` when that is the shortest resolving path at the diagnostic
site.

Human-facing `message`, `help`, and related labels MAY preserve an accessible
source alias or a more concise spelling. That presentation choice MUST NOT
change a structured type field.

### Canonical Structured Signature Rendering

Signature-valued diagnostic `data` fields use the declaration form
`fn name[generics](parameters) -> result` when binders exist and
`fn name(parameters) -> result` otherwise, with a trailing `!` on the callable
name when the callable is bang. They obey these rules:

1. Generic binders are alpha-renamed by position to `T0`, `T1`, and so on.
   Each binder's normalized bounds are sorted by canonical rendered trait
   reference. A fixed catalog specialization argument is not a binder; it
   retains its canonical type spelling.
2. Callable and value-parameter names are retained because they identify named
   call and declaration contracts. Whitespace is exactly one space after `fn`,
   each comma, and each parameter or bound colon, plus one space on each side
   of `->` and `+`. Catalog-only parameter markers use the spaces shown in Rule
   3; no other interior whitespace is emitted.
3. A resolved catalog callable retains applicability-relevant parameter markers
   as `named p: T`, `items: T...`, or `items: named T...`. A catalog callback
   parameter retains its normalized `callback[kind](...) -> R` spelling because
   not every callback kind has a source `Fn` type. Catalog default expressions
   and `requires` clauses are omitted: these fields are type-signature
   projections, not complete catalog declarations.
4. Every component type uses Canonical Structured Type Rendering. Code-specific
   semantic normalization happens first: for example, trait/impl comparison
   substitutes associated bindings before either signature is rendered.
5. Contextual `Self` that denotes the declaration's current owner is rendered
   as `Self`; this presentation rebinding does not undo semantic substitution.
6. A sequence of signatures in `candidates` is joined by `; ` in the
   diagnostic's declared candidate order.

Signature rendering is linear in the already-resolved signature size and MUST
NOT add a new search or inference phase.

### Type-System Detail Rendering

A plain-text renderer MUST expose the required type-system comparison data; a
generic message header alone is insufficient.

1. `expected_type`/`actual_type`, `left_type`/`right_type`, and
   `expected_signature`/`actual_signature` pairs MUST render both values.
2. A failed trait obligation MUST render its required `trait` and `type`.
3. An arity mismatch MUST render `expected_count` and `actual_count`.
4. A conditionally shaped diagnostic MUST render the active detail selected by
   `reason`, including `expected_family` or `candidates` when required.
5. Values MAY appear in the message or in labeled detail lines after the
   primary location. The renderer MAY choose labels suited to the diagnostic,
   but it MUST preserve the canonical structured values exactly.

This requirement does not add the rendered values to the per-code `message`
contract and does not constrain styling or explanatory prose.
