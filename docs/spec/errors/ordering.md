# Emission and Ordering

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Deterministic Ordering

When emitting multiple diagnostics in one run, order MUST be deterministic:

1. Source-bound primary spans precede `unavailable` primary spans.
2. Source-bound spans sort by `source`, `bytes.start`, then `bytes.end`.
3. Next sort by `severity` with `error` before `warning` before `note`.
4. Finally sort by lexical `code`.

`source` uses the canonical lexical text order from
`../language/invariants.md`. Derived text coordinates MUST NOT affect ordering;
the same physical source span has one order whether or not `text` is available.


### Diagnostic Roots and Poisoned Facts

These rules apply to compile-time error diagnostics. They distinguish failures
in the source from failures caused only by continuing analysis after an earlier
failure.

1. A diagnostic root is one failed normative obligation identified by its rule
   and code-specific blame identity. That identity is the offending source
   occurrence when one exists, the required zero-width insertion point for a
   missing construct, or the failing input-boundary instance for a diagnostic
   such as a missing package artifact. The failure MUST be establishable without
   facts made unavailable solely by an earlier error.
2. Separate blame identities that fail the same rule are separate roots. The
   same failed obligation reached again through another traversal or compiler
   phase remains one root.
3. After emitting a root diagnostic, an implementation MAY substitute an
   internal poisoned fact so that analysis can continue. A poisoned fact records
   that a prerequisite is unavailable because at least one root diagnostic was
   already emitted. Its representation and any opaque root handle are
   implementation details, not fields in the diagnostic record.
4. A candidate error is derivative when its rule cannot be decided without a
   poisoned fact and the failure cannot be established from the unaffected
   prerequisites alone. An implementation MUST NOT emit a separate diagnostic
   for a derivative error or emit the same root diagnostic more than once in one
   compilation.
5. A failure is independently actionable when its rule can be established from
   unaffected prerequisites. It remains independent when it appears in the same
   expression, statement, declaration, or source unit as another root.
6. A poisoned fact MUST NOT satisfy a language obligation, prove a program
   valid, or permit a successful compilation. It exists only to support bounded
   diagnostic recovery.
7. The retained error set consists of root diagnostics left after code-specific
   selection and any configured diagnostic limit. Emission ordering applies
   this set together with any retained warnings and notes; single-primary
   projection considers its errors only.


### Recovery Breadth

1. The v0.1 baseline does not require an implementation to discover every
   independent source error. Beyond the primary-failure requirement below and a
   diagnostic explicitly required by `diag-conformance-v1`, recovery breadth is
   implementation-defined.
2. After a root diagnostic, an implementation MAY stop the affected analysis
   region or resume at a recovery boundary. A later phase MAY skip a construct
   when that phase's required input facts are poisoned or structurally
   unavailable. Skipping such a region limits discovery; it is not suppression
   of a diagnostic that the implementation established.
3. Once recovery reaches a boundary whose required facts are unaffected, the
   implementation MAY continue checking that region. If a performed check
   establishes an independently actionable failure, the implementation MUST
   retain that root unless a code-specific selection rule or configured
   diagnostic limit excludes it. The root and derivative rules above apply
   after every recovery.
4. Implementations SHOULD recover at stable grammar boundaries, including
   top-level declarations and statement-list entries, and SHOULD continue across
   declarations or function bodies whose required signatures and structure are
   available. This is an implementation-quality target, not a minimum
   diagnostic-count requirement.
5. An implementation MAY apply a finite diagnostic limit. The configured limit
   and its truncation behavior MUST be deterministic for the same source,
   compiler options, and implementation revision. v0.1 does not prescribe a
   numeric limit.


### Code-specific Selection

1. A code-specific validation-precedence rule selects among failures before the
   retained diagnostics are ordered. This section does not override that rule.
2. A subsystem MAY define a stricter reporting boundary, such as one primary
   error per analysis instance. Such a boundary MUST be explicit in the
   subsystem's normative rules; an implementation MUST NOT invent one from its
   traversal order.
3. A derivative error is omitted rather than converted into a generic `note` or
   `related` span. Those fields remain optional, code-specific context for a
   retained diagnostic under `model.md`.
4. When a subsystem defines ordered validation stages for one analysis instance,
   a failure selected by an earlier stage MUST take precedence over every
   later-stage failure in that instance. Source position and implementation
   traversal order MUST NOT reverse that stage order. This rule does not require
   recovery after the earlier failure or exhaustive discovery within a stage.
5. When an enclosing runtime boundary converts an inner fatal outcome into a
   different outer outcome, that projection MUST be stated by the boundary's
   normative rules. It is boundary semantics, not diagnostic discovery order;
   implementations MUST NOT choose between the inner and outer codes by timing.


### Related Span Ordering

When one diagnostic emits multiple related spans, their sequence MUST be
deterministic.

1. A code-specific normative rule MAY prescribe semantic order, such as a
   cause chain or cycle path. An implementation MUST NOT select such an override
   on its own.
2. Otherwise, spans MUST use ascending source order by `span.source`,
   `span.bytes.start`, `span.bytes.end`, then `label`.
3. `source` and `label` use the canonical lexical text order from
   `../language/invariants.md`; filesystem-backed source identities use
   `Path.to_str()`.
4. Derived text coordinates MUST NOT affect related-span order.
5. A renderer MUST preserve the resulting sequence and MUST NOT reorder related
   spans by compiler traversal or discovery order.


### Primary Failure Requirement

For any failed compilation, at least one `error` diagnostic MUST be emitted.

When a boundary projects one primary compile diagnostic from a retained set, it
MUST select the first `error` after applying the deterministic ordering above.
This projection does not imply that the implementation emitted only one error.

For any fallible runtime/stdlib operation returning `Err(...)`, an implementation MUST return the error value and SHOULD emit a corresponding diagnostic. If emitted, the diagnostic MUST satisfy this document.


### Conformance Profile

Conformance runners MAY enable diagnostics profile `diag-conformance-v1`.

If `diag-conformance-v1` is enabled, diagnostics in this specification marked REQUIRED for that profile MUST be emitted.

The profile uses focused primary-diagnostic assertions. It does not require a
complete diagnostic set, a minimum recovery breadth, or a particular diagnostic
limit. Passing its required fixtures MUST NOT be cited as evidence that an
implementation performs multi-error recovery.
