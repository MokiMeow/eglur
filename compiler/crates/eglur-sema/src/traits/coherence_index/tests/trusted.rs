use super::*;

fn environment(
    count: usize,
    same_head: bool,
) -> (ExternalNameEnvironment, TrustedSignatureEnvironment) {
    let module = ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("probe")]).unwrap();
    let mark = ExternalDefId::new("std::probe::Mark").unwrap();
    let mut names = vec![
        ExternalDeclaration::new(
            mark.clone(),
            module.clone(),
            NameKey::new("Mark", false).unwrap(),
            DeclarationKind::Trait,
        )
        .unwrap(),
    ];
    let mut signatures = vec![TrustedDeclarationSignature::new(
        mark.clone(),
        TrustedSignatureKind::Trait {
            closed: false,
            generic_parameters: Box::new([TrustedGenericParameter {
                name: "A".into(),
                bounds: Box::new([]),
            }]),
            associated_types: Box::new([]),
        },
    )];
    for index in 0..count {
        for role in ["Host", "Arg"] {
            if role == "Host" && same_head && index != 0 {
                continue;
            }
            let id = ExternalDefId::new(format!("std::probe::{role}{index}")).unwrap();
            names.push(
                ExternalDeclaration::new(
                    id.clone(),
                    module.clone(),
                    NameKey::new(format!("{role}{index}"), false).unwrap(),
                    DeclarationKind::Struct,
                )
                .unwrap(),
            );
            signatures.push(TrustedDeclarationSignature::new(
                id,
                TrustedSignatureKind::Nominal {
                    generic_parameters: Box::new([]),
                },
            ));
        }
        let host = ExternalDefId::new(format!(
            "std::probe::Host{}",
            if same_head { 0 } else { index }
        ))
        .unwrap();
        let arg = ExternalDefId::new(format!("std::probe::Arg{index}")).unwrap();
        signatures.push(TrustedDeclarationSignature::new(
            ExternalDefId::new(format!("std::probe::impl{index}")).unwrap(),
            TrustedSignatureKind::Impl {
                negative: false,
                generic_parameters: Box::new([]),
                trait_ref: TrustedTraitExpr {
                    declaration: mark.clone(),
                    arguments: Box::new([TrustedTypeExpr::Nominal {
                        head: arg,
                        arguments: Box::new([]),
                    }]),
                },
                target: TrustedTypeExpr::Nominal {
                    head: host,
                    arguments: Box::new([]),
                },
                associated_bindings: Box::new([]),
            },
        ));
    }
    (
        ExternalNameEnvironment::new([module], names, [mark]).unwrap(),
        TrustedSignatureEnvironment::new(signatures).unwrap(),
    )
}

#[test]
fn native_trusted_source_seed_and_checked_passes_only_classify_compatible_heads() {
    for count in [1, 8, 32, 128] {
        for same_head in [false, true] {
            let (external, trusted) = environment(count, same_head);
            let expected = if same_head {
                count * (count - 1) / 2
            } else {
                0
            };
            let text = "struct LocalHost {}\nimpl Mark[LocalHost] for LocalHost {}\n";
            with_semantics(
                text,
                &external,
                Some(&trusted),
                |declarations, signatures, sources, diagnostics, signature_work| {
                    assert!(signature_work <= expected);
                    let mut admitted = 0;
                    for declaration in trusted.declarations() {
                        if matches!(declaration.kind(), TrustedSignatureKind::Impl { .. }) {
                            let target = CanonicalTarget::External(declaration.id().clone());
                            assert!(matches!(
                                signatures.impl_header(&target),
                                FactState::Known(_)
                            ));
                            assert!(matches!(
                                signatures.declaration(&target),
                                FactState::Known(DeclarationSignature::Impl { .. })
                            ));
                            admitted += 1;
                        }
                    }
                    assert_eq!(admitted, count);
                    let local = all_source_headers(declarations, signatures);
                    assert!(local.iter().all(|header| matches!(
                        signatures.impl_header(header.declaration()),
                        FactState::Known(_)
                    )));
                    reset_comparisons();
                    performed(
                        build_trait_environment(
                            declarations,
                            sources,
                            &Support,
                            signatures,
                            diagnostics,
                        )
                        .unwrap(),
                    );
                    assert!(comparisons() <= expected);
                    assert!(
                        std::mem::take(diagnostics)
                            .finalize(None)
                            .diagnostics()
                            .is_empty()
                    );
                },
            )
            .unwrap();
        }
    }
}

fn conflicting_environment(
    unconstrained: bool,
) -> (ExternalNameEnvironment, TrustedSignatureEnvironment) {
    let (external, base) = environment(2, false);
    let mut declarations = base.declarations().cloned().collect::<Vec<_>>();
    let mark = ExternalDefId::new("std::probe::Mark").unwrap();
    for (index, head) in [0, 1, 1, 0].into_iter().enumerate() {
        declarations.push(TrustedDeclarationSignature::new(
            ExternalDefId::new(format!("std::probe::ordered{index}")).unwrap(),
            TrustedSignatureKind::Impl {
                negative: false,
                generic_parameters: Box::new([]),
                trait_ref: TrustedTraitExpr {
                    declaration: mark.clone(),
                    arguments: Box::new([TrustedTypeExpr::Primitive(PrimitiveType::Bool)]),
                },
                target: TrustedTypeExpr::Nominal {
                    head: ExternalDefId::new(format!("std::probe::Host{head}")).unwrap(),
                    arguments: Box::new([]),
                },
                associated_bindings: Box::new([]),
            },
        ));
    }
    if unconstrained {
        declarations.push(TrustedDeclarationSignature::new(
            ExternalDefId::new("std::probe::zzz").unwrap(),
            TrustedSignatureKind::Impl {
                negative: false,
                generic_parameters: Box::new([TrustedGenericParameter {
                    name: "T".into(),
                    bounds: Box::new([]),
                }]),
                trait_ref: TrustedTraitExpr {
                    declaration: mark,
                    arguments: Box::new([TrustedTypeExpr::Primitive(PrimitiveType::Bool)]),
                },
                target: TrustedTypeExpr::Nominal {
                    head: ExternalDefId::new("std::probe::Host0").unwrap(),
                    arguments: Box::new([]),
                },
                associated_bindings: Box::new([]),
            },
        ));
    }
    (
        external,
        TrustedSignatureEnvironment::new(declarations).unwrap(),
    )
}

#[test]
fn trusted_unconstrained_parameters_precede_all_coherence_and_first_left_owner_is_preserved() {
    let (external, trusted) = conflicting_environment(true);
    let error = with_semantics("", &external, Some(&trusted), |_, _, _, _, _| ()).unwrap_err();
    assert!(
        matches!(error, SignatureResolutionError::Trusted(TrustedSignatureError::UnconstrainedImplParameter { declaration, .. }) if declaration == ExternalDefId::new("std::probe::zzz").unwrap())
    );
    assert_eq!(comparisons(), 0);

    let (external, trusted) = conflicting_environment(false);
    let error = with_semantics("", &external, Some(&trusted), |_, _, _, _, _| ()).unwrap_err();
    assert_eq!(
        error,
        SignatureResolutionError::Trusted(TrustedSignatureError::CoherenceViolation(
            ExternalDefId::new("std::probe::ordered0").unwrap()
        ))
    );
}
