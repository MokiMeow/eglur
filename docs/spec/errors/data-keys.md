# Structured Data Keys

Scope: Normative for v0.1 unless this file or a section states otherwise.

When `data` is present for codes in this spec, keys SHOULD use the following canonical names:

`diagnostic-contracts.yaml` is the normative source for per-code required,
emit-if-present, conditional, and exact-value key contracts.

Numeric diagnostic payload values use the diagnostic model's `i64` scalar even when the named quantity is semantically non-negative.
This does not override `Nat` or `PositiveNat` requirements for user-callable API parameters.

1. `index` (0-based i64).
2. `phase_index` (0-based i64; ordered phase position in phase-based diagnostics).
3. `caller`, `callee` (Str).
4. `axis`, `rank`.
5. `name`.
6. `candidates`.
7. `expected`, `actual` (code-specific non-type values in parser and value
   contracts; these keys do not carry type spellings).
8. `expected_type`, `actual_type` (canonical diagnostic type spellings).
9. `expected_count`, `minimum_count`, `actual_count` (non-negative i64 arities or cardinalities).
10. `expected_signature`, `actual_signature` (canonical diagnostic callable
    signatures).
11. `expected_family` (stable identifier for a closed accepted type family).
12. `expected_axes`, `actual_axes`.
13. `operation`.
14. `key`.
15. `field`, `property`, `accessor`, `parameter`, `method`, `binding`.
16. `struct`, `variant`, `constructor`, `associated_type`.
17. `pattern`, `pattern_kind`.
18. `expected_form`, `actual_form`.
19. `dtype`.
20. `src_dtype`, `dst_dtype`.
21. `argnum`.
22. `profile`.
23. `backend`.
24. `device`.
25. `driver`, `runtime`, `abi`.
26. `thread_config`.
27. `determinism_context`.
28. `dim`, `position`.
29. `value`.
30. `source`.
31. `boundary`.
32. `dependency`, `package`, `version_spec`, `versions`.
33. `attribute`, `target`, `function`, `trait`, `item_kind`, `expected_kind`,
    `actual_kind`.
34. `type`, `left_type`, `right_type`.
35. `symbol`.
36. `reason`, `related`, `cycle`.
37. `domain`.
38. `output_index` (0-based i64; right-hand-side output position).
39. `label`, `input_labels`, `output_labels` (Str; one schema label or a deterministic label-list summary).
40. `expected_dim`, `actual_dim` (i64).
41. `source_rank`, `consuming_items`, `count` (non-negative i64 indexing-structure quantities).
42. `path_hex`, `entry_kind`, `resource`, `limit`, `observed_at_least`.

Implementations MAY include additional keys.

### Required-Key Policy

1. Required `data` keys for a code SHOULD be the smallest set that preserves
   the failure's semantic shape. A comparison pair MAY additionally require an
   operation, declaration identity, or conditional-shape discriminator.
2. Implementations MAY emit richer supplemental keys.
3. Supplemental keys MUST NOT change the semantic meaning of required keys.

### Type-System Key Families

Type-system diagnostics use keys according to the compared semantic facts:

1. A directional assignability failure uses `expected_type` and `actual_type`.
2. A symmetric operand failure or a source-ordered common-type failure uses
   `left_type` and `right_type`.
3. A unary operand failure uses `actual_type`; it MUST NOT duplicate that type
   into an invented left/right pair.
4. A callable-declaration comparison uses `expected_signature` and
   `actual_signature`. Associated identity fields such as `method` and
   `accessor` retain the declared callable name, including a trailing `!`.
5. An exact arity comparison uses `expected_count` and `actual_count`; a
   lower-bound comparison uses `minimum_count` and `actual_count`. Neither form
   may serialize counts through type-valued keys.
6. A failed trait obligation uses `trait` and `type`; `trait` is the complete
   normalized trait reference and `type` is the obligation subject.
7. A structural pattern failure uses `pattern_kind` plus the contextual type or
   count keys required by that diagnostic. It MUST NOT invent a type for a
   tuple, Unit, wildcard, or other untyped pattern form.
8. `reason` selects conditional keys only when one diagnostic code covers
   multiple shapes that cannot share truthful fields. A producer MUST NOT emit
   an arbitrary `expected_type` merely to avoid the conditional shape.
9. A constructor-target comparison uses `expected_kind` and `actual_kind`.
   Both values use this closed normalized vocabulary: `function`,
   `variant_constructor`, `trait`, `impl`, `struct`, `nominal_type`, `primitive_type`, `enum`,
   `field`, `type_alias`, `const`, `associated_type`,
   `associated_type_binding`, `property`, and `generic_parameter`.
   Implementations MUST NOT expose internal declaration or signature enum names
   through these keys.

Code spelling alone does not create a pair contract. An unsupported-position,
underdetermined-type, or whole-declaration policy diagnostic uses its
code-specific offending fact or identity; it MUST NOT synthesize an
`expected_type` or `expected_signature` when no unique counterpart exists.

All type- and signature-valued strings in these families follow the canonical
structured-data rendering in `rendering.md`.

`candidates` is a semicolon-and-space-separated sequence when it carries
canonical callable signatures. The sequence follows the diagnostic's declared
candidate order; a producer MUST NOT encode an unordered set in this field.
