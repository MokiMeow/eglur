# Extension-profile Compile-time Error Codes

Scope: Normative for v0.1 unless this file or a section states otherwise.

This section defines compile-channel codes for non-baseline extension profiles.

`diagnostic-contracts.yaml` is normative for severity, message, and `data`
fields. The per-code field lists below are explanatory summaries; trigger and
channel rules remain normative prose.

These codes are excluded from required v0.1 baseline conformance suites unless an extension suite explicitly enables the matching profile.

### `DERIVE_EXPANSION_FORBIDDEN`

Severity: `error`

Trigger: derive expansion attempts to emit declarations outside v0.1 derive boundary (anything other than `impl Trait for Type` for the annotated type).

Message contract summary:

1. `derive`.
2. `expansion`.
3. `forbidden`.

Data contract summary (if `data` is present):

1. `trait`.
2. `item_kind`.


### `OWNERSHIP_PROFILE_DISABLED`

Severity: `error`

Trigger: ownership surface usage is detected while ownership profile is disabled, including:

1. `Own[...]` type constructor usage.
2. Ownership operators/syntax (`own`, `move`, `drop_own`).

Message contract summary:

1. `ownership`.
2. `profile`.
3. `disabled`.

Data contract summary (if `data` is present):

1. `profile`.
