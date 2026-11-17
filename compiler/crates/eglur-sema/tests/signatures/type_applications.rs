use super::*;
use eglur_sema::{PrimitiveType, TraitEnvironmentError};

fn checked_environment(
    argument: TrustedTypeExpr,
    symbolic_bound: bool,
    concrete_impl: bool,
    trait_application: bool,
) -> Result<bool, TraitEnvironmentError> {
    let module = ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("bounds")]).unwrap();
    let marker = ExternalDefId::new("std::bounds::Mark").unwrap();
    let guarded = ExternalDefId::new("std::bounds::Guarded").unwrap();
    let uses = ExternalDefId::new("std::bounds::Uses").unwrap();
    let probe = ExternalDefId::new("std::bounds::probe").unwrap();
    let marker_ref = TrustedTraitExpr {
        declaration: marker.clone(),
        arguments: Box::new([]),
    };
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [
            (marker.clone(), "Mark", DeclarationKind::Trait),
            (guarded.clone(), "Guarded", DeclarationKind::OpaqueType),
            (uses.clone(), "Uses", DeclarationKind::Trait),
            (probe.clone(), "probe", DeclarationKind::Function),
        ]
        .into_iter()
        .map(|(id, name, kind)| {
            ExternalDeclaration::new(id, module.clone(), NameKey::new(name, false).unwrap(), kind)
                .unwrap()
        }),
        [],
    )
    .unwrap();
    let parameter = TrustedGenericParameter {
        name: "T".into(),
        bounds: Box::new([marker_ref.clone()]),
    };
    let mut declarations = vec![
        TrustedDeclarationSignature::new(
            marker.clone(),
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([]),
                associated_types: Box::new([]),
            },
        ),
        TrustedDeclarationSignature::new(
            guarded.clone(),
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([parameter.clone()]),
            },
        ),
        TrustedDeclarationSignature::new(
            uses.clone(),
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([parameter]),
                associated_types: Box::new([]),
            },
        ),
        TrustedDeclarationSignature::new(
            probe,
            TrustedSignatureKind::Callable {
                bang: false,
                generic_parameters: Box::new([TrustedGenericParameter {
                    name: "T".into(),
                    bounds: if trait_application {
                        Box::new([TrustedTraitExpr {
                            declaration: uses,
                            arguments: Box::new([argument.clone()]),
                        }])
                    } else if symbolic_bound {
                        Box::new([marker_ref.clone()])
                    } else {
                        Box::new([])
                    },
                }]),
                parameters: Box::new([]),
                result: if trait_application {
                    TrustedTypeExpr::Primitive(PrimitiveType::Unit)
                } else {
                    TrustedTypeExpr::Nominal {
                        head: guarded,
                        arguments: Box::new([argument]),
                    }
                },
                requirements: Box::new([]),
            },
        ),
    ];
    if concrete_impl {
        declarations.push(TrustedDeclarationSignature::new(
            ExternalDefId::new("std::bounds::mark_integer").unwrap(),
            TrustedSignatureKind::Impl {
                negative: false,
                generic_parameters: Box::new([]),
                trait_ref: marker_ref,
                target: TrustedTypeExpr::Primitive(PrimitiveType::I64),
                associated_bindings: Box::new([]),
            },
        ));
    }
    let trusted = TrustedSignatureEnvironment::new(declarations).unwrap();
    checked_catalog(&external, &trusted)
}

fn checked_catalog(
    external: &ExternalNameEnvironment,
    trusted: &TrustedSignatureEnvironment,
) -> Result<bool, TraitEnvironmentError> {
    let source = "fn local() -> Unit = ()\n";
    let mut support = CapabilitySet::signatures();
    support.0.insert(CompileCapability::Typecheck(
        TypecheckCapability::ResolveTraitEnvironment,
    ));
    let mut analysis = analyze(source, &support, Some(external), Some(trusted));
    assert!(diagnostic_codes(&analysis.diagnostics).is_empty());
    let prepared = prepare("main.eglur", source);
    let sources = [SemanticSource::new(
        prepared.file,
        prepared.module,
        Some(&prepared.hir),
        prepared.spans,
        [],
        None,
    )
    .unwrap()];
    let mut diagnostics = DiagnosticStore::new();
    let result = eglur_sema::build_trait_environment(
        &analysis.declarations,
        &sources,
        &support,
        analysis.signatures.as_mut().unwrap(),
        &mut diagnostics,
    )?;
    assert!(matches!(
        diagnostics.finalize(None),
        DiagnosticSnapshot::Accepted(_)
    ));
    Ok(matches!(result.into_parts().0, PhaseProduct::Performed(_)))
}

#[test]
fn trusted_bound_contracts_check_every_bound_and_reject_duplicates() {
    let module = ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("bounds")]).unwrap();
    let alpha = ExternalDefId::new("std::bounds::Alpha").unwrap();
    let zulu = ExternalDefId::new("std::bounds::Zulu").unwrap();
    let guarded = ExternalDefId::new("std::bounds::Guarded").unwrap();
    let probe = ExternalDefId::new("std::bounds::probe").unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [
            (alpha.clone(), "Alpha", DeclarationKind::Trait),
            (zulu.clone(), "Zulu", DeclarationKind::Trait),
            (guarded.clone(), "Guarded", DeclarationKind::OpaqueType),
            (probe.clone(), "probe", DeclarationKind::Function),
        ]
        .into_iter()
        .map(|(id, name, kind)| {
            ExternalDeclaration::new(id, module.clone(), NameKey::new(name, false).unwrap(), kind)
                .unwrap()
        }),
        [],
    )
    .unwrap();

    for (implementations, duplicate) in [(0, false), (1, false), (2, false), (3, false), (3, true)]
    {
        let mut declarations = Vec::new();
        let mut bounds = Vec::new();
        for (index, marker) in [&zulu, &alpha].into_iter().enumerate() {
            let trait_ref = TrustedTraitExpr {
                declaration: marker.clone(),
                arguments: Box::new([]),
            };
            bounds.push(trait_ref.clone());
            declarations.push(TrustedDeclarationSignature::new(
                marker.clone(),
                TrustedSignatureKind::Trait {
                    closed: false,
                    generic_parameters: Box::new([]),
                    associated_types: Box::new([]),
                },
            ));
            if implementations & (1 << index) != 0 {
                declarations.push(TrustedDeclarationSignature::new(
                    ExternalDefId::new(format!("std::bounds::impl{index}")).unwrap(),
                    TrustedSignatureKind::Impl {
                        negative: false,
                        generic_parameters: Box::new([]),
                        trait_ref,
                        target: TrustedTypeExpr::Primitive(PrimitiveType::I64),
                        associated_bindings: Box::new([]),
                    },
                ));
            }
        }
        if duplicate {
            bounds.push(bounds[0].clone());
        }
        declarations.push(TrustedDeclarationSignature::new(
            guarded.clone(),
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([TrustedGenericParameter {
                    name: "T".into(),
                    bounds: bounds.into_boxed_slice(),
                }]),
            },
        ));
        declarations.push(TrustedDeclarationSignature::new(
            probe.clone(),
            TrustedSignatureKind::Callable {
                bang: false,
                generic_parameters: Box::new([]),
                parameters: Box::new([]),
                result: TrustedTypeExpr::Nominal {
                    head: guarded.clone(),
                    arguments: Box::new([TrustedTypeExpr::Primitive(PrimitiveType::I64)]),
                },
                requirements: Box::new([]),
            },
        ));
        let trusted = TrustedSignatureEnvironment::new(declarations).unwrap();
        if duplicate {
            assert!(matches!(
                try_analyze(
                    "fn local() -> Unit = ()\n",
                    &CapabilitySet::signatures(),
                    Some(&external),
                    Some(&trusted),
                ),
                Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::DuplicateBound { .. }
                ))
            ));
        } else if implementations == 3 {
            assert!(checked_catalog(&external, &trusted).unwrap());
        } else {
            assert!(matches!(
                checked_catalog(&external, &trusted),
                Err(TraitEnvironmentError::InvalidTrustedTypeApplication { .. })
            ));
        }
    }
}

#[test]
fn trusted_type_applications_require_concrete_or_exact_declared_evidence() {
    for trait_application in [false, true] {
        let result = checked_environment(
            TrustedTypeExpr::Primitive(PrimitiveType::I64),
            false,
            false,
            trait_application,
        );
        assert!(
            matches!(
                result,
                Err(TraitEnvironmentError::InvalidTrustedTypeApplication { .. })
            ),
            "{result:?}"
        );
        assert!(
            checked_environment(
                TrustedTypeExpr::Primitive(PrimitiveType::I64),
                false,
                true,
                trait_application
            )
            .unwrap()
        );
    }
    assert!(checked_environment(TrustedTypeExpr::GenericParameter(0), true, false, false).unwrap());
    assert!(matches!(
        checked_environment(TrustedTypeExpr::GenericParameter(0), false, false, false),
        Err(TraitEnvironmentError::InvalidTrustedTypeApplication { .. })
    ));
}

#[test]
fn trusted_constructor_bounds_use_the_complete_parameter_environment() {
    let module = ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("bounds")]).unwrap();
    let marker = ExternalDefId::new("std::bounds::Mark").unwrap();
    let uses = ExternalDefId::new("std::bounds::Uses").unwrap();
    let ok = ExternalDefId::new("std::bounds::Ok").unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [
            (marker.clone(), "Mark", DeclarationKind::Trait),
            (uses.clone(), "Uses", DeclarationKind::Trait),
            (ok.clone(), "Ok", DeclarationKind::IntrinsicConstructor),
        ]
        .into_iter()
        .map(|(id, name, kind)| {
            ExternalDeclaration::new(id, module.clone(), NameKey::new(name, false).unwrap(), kind)
                .unwrap()
        }),
        [],
    )
    .unwrap();
    for has_bound in [true, false] {
        let marker_ref = TrustedTraitExpr {
            declaration: marker.clone(),
            arguments: Box::new([]),
        };
        let trusted = TrustedSignatureEnvironment::new([
            TrustedDeclarationSignature::new(
                marker.clone(),
                TrustedSignatureKind::Trait {
                    closed: false,
                    generic_parameters: Box::new([]),
                    associated_types: Box::new([]),
                },
            ),
            TrustedDeclarationSignature::new(
                uses.clone(),
                TrustedSignatureKind::Trait {
                    closed: false,
                    generic_parameters: Box::new([TrustedGenericParameter {
                        name: "A".into(),
                        bounds: Box::new([marker_ref.clone()]),
                    }]),
                    associated_types: Box::new([]),
                },
            ),
            TrustedDeclarationSignature::new(
                ok.clone(),
                TrustedSignatureKind::Constructor {
                    generic_parameters: Box::new([
                        TrustedGenericParameter {
                            name: "T".into(),
                            bounds: Box::new([TrustedTraitExpr {
                                declaration: uses.clone(),
                                arguments: Box::new([TrustedTypeExpr::GenericParameter(1)]),
                            }]),
                        },
                        TrustedGenericParameter {
                            name: "E".into(),
                            bounds: if has_bound {
                                Box::new([marker_ref])
                            } else {
                                Box::new([])
                            },
                        },
                    ]),
                    payload: TrustedConstructorPayload::Positional(Box::new([
                        TrustedTypeExpr::GenericParameter(0),
                    ])),
                    result: TrustedTypeExpr::Intrinsic {
                        head: IntrinsicNominal::Res,
                        arguments: Box::new([
                            TrustedTypeExpr::GenericParameter(0),
                            TrustedTypeExpr::GenericParameter(1),
                        ]),
                    },
                },
            ),
        ])
        .unwrap();
        let result = checked_catalog(&external, &trusted);
        if has_bound {
            assert!(result.unwrap());
        } else {
            assert!(
                matches!(
                    result,
                    Err(TraitEnvironmentError::InvalidTrustedTypeApplication { .. })
                ),
                "{result:?}"
            );
        }
    }
}
