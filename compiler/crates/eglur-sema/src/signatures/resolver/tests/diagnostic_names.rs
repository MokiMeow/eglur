use super::*;
use crate::type_rendering::DiagnosticTypeRenderingError;
use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, PrimarySpan, Severity,
};

fn poison(diagnostics: &mut DiagnosticStore) -> Poison {
    let diagnostic = Diagnostic::new(
        DiagnosticCode::new("TRAIT_BOUND_NOT_SATISFIED").unwrap(),
        Severity::Error,
        DiagnosticPhase::new("type").unwrap(),
        "trait bound is not satisfied",
        PrimarySpan::Unavailable,
    );
    diagnostics
        .record_failure(DiagnosticFailure::Root(
            DiagnosticRoot::boundary(BoundaryRootKey::new("test-root").unwrap(), diagnostic)
                .unwrap(),
        ))
        .unwrap()
}

fn with_signatures(inspect: impl FnOnce(&DeclarationCollection, DeclarationSignatures)) {
    with_resolver(
        &[concat!(
            "trait Mark {}\n",
            "fn pick[SourceName: Mark](value: SourceName) -> SourceName = value\n",
            "fn independent[OtherName](value: OtherName) -> OtherName = value\n",
        )],
        |resolver, sources| {
            let mut diagnostics = DiagnosticStore::new();
            let completed = performed(
                resolve_declaration_signatures(
                    resolver.collection,
                    resolver.imports,
                    sources,
                    &Support,
                    SignatureResolutionInputs::new(resolver.external_names, None),
                    &mut diagnostics,
                )
                .unwrap(),
            );
            inspect(resolver.collection, completed.into_parts().0);
        },
    );
}

#[test]
fn diagnostic_names_survive_both_use_site_poisons_without_reopening_bounds() {
    for application_poison in [true, false] {
        with_signatures(|collection, mut signatures| {
            let declarations = collection.modules().next().unwrap();
            let parameter = declarations
                .declarations()
                .find(|declaration| {
                    declaration
                        .name()
                        .is_some_and(|name| name.text() == "SourceName")
                })
                .unwrap();
            let owner = parameter.parent().unwrap();
            let parameter = GenericParameterId::Source(parameter.id());
            let FactState::Known(DeclarationSignature::Callable(callable)) =
                signatures.source_declaration(owner)
            else {
                panic!("pick has a callable signature");
            };
            let callable = signatures.callable(*callable).unwrap().clone();
            let parameter_type = signatures
                .types_mut()
                .generic_parameter(parameter.clone())
                .unwrap();
            let nested = signatures
                .types_mut()
                .nominal(
                    NominalHead::Intrinsic(IntrinsicNominal::Opt),
                    [parameter_type],
                )
                .unwrap();
            let renderer = DiagnosticTypeRenderer::new(
                collection,
                signatures.diagnostic_type_names(),
                &signatures,
                owner,
            );
            assert_eq!(renderer.render_type(nested).unwrap(), "Opt[SourceName]");
            assert_eq!(
                renderer.render_callable_signature(&callable).unwrap(),
                "fn pick[T0: Mark](value: T0) -> T0"
            );

            let mut diagnostics = DiagnosticStore::new();
            let poison = poison(&mut diagnostics);
            for target in [CanonicalTarget::Source(owner), generic_target(&parameter)] {
                if application_poison {
                    assert!(signatures.poison_type_application_owner(target, poison.clone()));
                } else {
                    assert!(signatures.poison_public_contract(target, poison.clone()));
                }
            }
            assert!(matches!(
                signatures.source_declaration(owner),
                FactState::Poisoned(_)
            ));
            assert!(matches!(
                signatures.generic_parameter(&parameter),
                FactState::Poisoned(_)
            ));
            assert!(matches!(
                signatures.generic_parameter_record(&parameter),
                FactState::Poisoned(_)
            ));
            assert_eq!(
                signatures.generic_parameter_name(&parameter),
                Some("SourceName")
            );
            assert_eq!(signatures.generic_parameter_bounds(&parameter), None);

            let renderer = DiagnosticTypeRenderer::new(
                collection,
                signatures.diagnostic_type_names(),
                &signatures,
                owner,
            );
            assert_eq!(renderer.render_type(nested).unwrap(), "Opt[SourceName]");
            assert!(matches!(
                renderer.render_callable_signature(&callable),
                Err(DiagnosticTypeRenderingError::UnavailableGenericParameter(found))
                    if found == parameter
            ));
            let independent = declarations
                .declarations()
                .find(|declaration| {
                    declaration
                        .name()
                        .is_some_and(|name| name.text() == "OtherName")
                })
                .unwrap();
            let independent = GenericParameterId::Source(independent.id());
            assert!(matches!(
                signatures.generic_parameter(&independent),
                FactState::Known(_)
            ));
            assert_eq!(
                signatures.generic_parameter_name(&independent),
                Some("OtherName")
            );
            assert_eq!(
                signatures.generic_parameter_bounds(&independent),
                Some(&[][..])
            );
        });
    }
}

#[test]
fn trusted_name_projection_preserves_identity_and_refuses_unavailable_rows() {
    with_signatures(|collection, mut signatures| {
        let owner = collection
            .modules()
            .next()
            .unwrap()
            .declarations()
            .next()
            .unwrap()
            .id();
        let mut diagnostics = DiagnosticStore::new();
        let poison = poison(&mut diagnostics);
        for index in 0..4 {
            let parameter = GenericParameterId::Trusted {
                owner: ExternalDefId::new("catalog::pick").unwrap(),
                index,
            };
            let fact = match index {
                0 => Some(SemanticFact::Known(GenericParameterSignature::new(
                    parameter.clone(),
                    "CatalogName".into(),
                    Vec::new(),
                ))),
                1 => Some(SemanticFact::Poisoned(poison.clone())),
                2 => Some(SemanticFact::NotApplicable),
                _ => None,
            };
            if let Some(fact) = fact {
                signatures
                    .generic_parameters
                    .insert(parameter.clone(), fact)
                    .unwrap();
            }
            let ty = signatures
                .types_mut()
                .generic_parameter(parameter.clone())
                .unwrap();
            let renderer = DiagnosticTypeRenderer::new(
                collection,
                signatures.diagnostic_type_names(),
                &signatures,
                owner,
            );
            if index == 0 {
                assert_eq!(
                    signatures.generic_parameter_name(&parameter),
                    Some("CatalogName")
                );
                assert_eq!(renderer.render_type(ty).unwrap(), "CatalogName");
            } else {
                assert_eq!(signatures.generic_parameter_name(&parameter), None);
                assert!(matches!(
                    renderer.render_type(ty),
                    Err(DiagnosticTypeRenderingError::UnavailableGenericParameter(found))
                        if found == parameter
                ));
            }
        }

        let not_a_parameter = GenericParameterId::Source(owner);
        assert_eq!(signatures.generic_parameter_name(&not_a_parameter), None);
    });
}
