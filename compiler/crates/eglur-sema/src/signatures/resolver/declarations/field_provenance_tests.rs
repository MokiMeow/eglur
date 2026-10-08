use std::cell::Cell;

use super::super::tests::with_resolver;
use super::*;
use crate::PrimitiveType;

thread_local! {
    static ORIGIN_INSPECTIONS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn observe_origin_inspection() {
    ORIGIN_INSPECTIONS.update(|count| count + 1);
}

fn variants(resolver: &SignatureResolver<'_, '_>) -> Vec<DefId> {
    resolver
        .collection
        .modules()
        .flat_map(|module| module.declarations())
        .filter(|declaration| declaration.kind() == DeclarationKind::EnumVariant)
        .map(Declaration::id)
        .collect()
}

#[test]
fn named_field_origin_recovery_is_bounded_and_retains_owner_and_payload_order() {
    for count in [1, 8, 32, 128] {
        for reversed in [false, true] {
            let order = if reversed {
                (0..count).rev().collect::<Vec<_>>()
            } else {
                (0..count).collect::<Vec<_>>()
            };
            let fields = order
                .iter()
                .map(|index| format!("f{index}:{}", if index % 2 == 0 { "i64" } else { "Bool" }))
                .collect::<Vec<_>>()
                .join(",");
            let text = format!(
                "enum First {{ Item {{ {fields} }} }}\nenum Second {{ Item {{ {fields} }} }}\n"
            );
            with_resolver(&[&text], |resolver, sources| {
                let source = &sources[0];
                for variant in variants(resolver) {
                    let expected = resolver
                        .child_declarations(variant, DeclarationKind::EnumVariantField)
                        .collect::<Vec<_>>();
                    assert_eq!(expected.len(), count);
                    ORIGIN_INSPECTIONS.set(0);
                    let Resolution::Known(DeclarationSignature::EnumVariant {
                        owner,
                        payload: EnumVariantPayloadSignature::Named(actual),
                        ..
                    }) = resolver
                        .resolve_source_enum_variant(variant, source, source.hir().unwrap())
                        .unwrap()
                    else {
                        panic!("fixture requires known named payload");
                    };
                    assert!(ORIGIN_INSPECTIONS.get() <= expected.len());
                    assert_eq!(
                        owner,
                        CanonicalTarget::Source(
                            resolver
                                .collection
                                .declaration(variant)
                                .unwrap()
                                .parent()
                                .unwrap()
                        )
                    );
                    assert_eq!(
                        actual.as_ref(),
                        expected
                            .iter()
                            .copied()
                            .map(CanonicalTarget::Source)
                            .collect::<Vec<_>>()
                    );
                    for (&id, &index) in expected.iter().zip(&order) {
                        let FactState::Known(DeclarationSignature::Field { name, ty }) =
                            resolver.declarations.state(&CanonicalTarget::Source(id))
                        else {
                            panic!("field published with recovered identity");
                        };
                        assert_eq!(name.text(), format!("f{index}"));
                        assert_eq!(
                            resolver.types.kind(*ty).unwrap(),
                            &TypeKind::Primitive(if index % 2 == 0 {
                                PrimitiveType::I64
                            } else {
                                PrimitiveType::Bool
                            })
                        );
                    }
                }
            });
        }
    }
}

#[test]
fn duplicate_field_spellings_keep_distinct_origin_identities_and_types() {
    for count in [2, 8, 32, 128] {
        let fields = (0..count)
            .map(|index| format!("same:{}", if index % 2 == 0 { "i64" } else { "Bool" }))
            .collect::<Vec<_>>()
            .join(",");
        let text = format!("enum E {{ Item {{ {fields} }} }}\n");
        with_resolver(&[&text], |resolver, sources| {
            let variant = variants(resolver)[0];
            let expected = resolver
                .child_declarations(variant, DeclarationKind::EnumVariantField)
                .collect::<Vec<_>>();
            ORIGIN_INSPECTIONS.set(0);
            let Resolution::Known(DeclarationSignature::EnumVariant {
                payload: EnumVariantPayloadSignature::Named(actual),
                ..
            }) = resolver
                .resolve_source_enum_variant(variant, &sources[0], sources[0].hir().unwrap())
                .unwrap()
            else {
                panic!("known payload before downstream admission");
            };
            assert!(ORIGIN_INSPECTIONS.get() <= expected.len());
            assert_eq!(
                actual.as_ref(),
                expected
                    .iter()
                    .copied()
                    .map(CanonicalTarget::Source)
                    .collect::<Vec<_>>()
            );
            for (index, &id) in expected.iter().enumerate() {
                let FactState::Known(DeclarationSignature::Field { name, ty }) =
                    resolver.declarations.state(&CanonicalTarget::Source(id))
                else {
                    panic!("field retains its own fact");
                };
                assert_eq!(name.text(), "same");
                assert_eq!(
                    resolver.types.kind(*ty).unwrap(),
                    &TypeKind::Primitive(if index % 2 == 0 {
                        PrimitiveType::I64
                    } else {
                        PrimitiveType::Bool
                    })
                );
            }
        });
    }
}

#[test]
fn rejected_variant_visibility_does_not_prepare_field_origin_lookup() {
    with_resolver(
        &["enum E { pub Item { first:i64, second:Bool } }\n"],
        |resolver, sources| {
            let variant = variants(resolver)[0];
            ORIGIN_INSPECTIONS.set(0);
            assert!(matches!(
                resolver
                    .resolve_source_enum_variant(variant, &sources[0], sources[0].hir().unwrap())
                    .unwrap(),
                Resolution::Poisoned(_)
            ));
            assert_eq!(ORIGIN_INSPECTIONS.get(), 0);
            for id in resolver.child_declarations(variant, DeclarationKind::EnumVariantField) {
                assert!(matches!(
                    resolver.declarations.state(&CanonicalTarget::Source(id)),
                    FactState::Unperformed
                ));
            }
        },
    );
}
