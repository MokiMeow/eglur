use super::*;
use eglur_sema::{
    PrimitiveType, TraitEnvironment, TraitNormalization, TraitObligation, TraitProof, TypeId,
};

fn environment(
    target: TrustedTypeExpr,
    value: TrustedTypeExpr,
    generic: bool,
    base: TrustedTypeExpr,
) -> (Analysis, TraitEnvironment, TypeId, TraitObligation) {
    let module =
        ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("identity")]).unwrap();
    let identity = ExternalDefId::new("std::identity::Identity").unwrap();
    let item = ExternalDefId::new("std::identity::Identity::Item").unwrap();
    let implementation = ExternalDefId::new("std::identity::impl").unwrap();
    let probe = ExternalDefId::new("std::identity::probe").unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [
            ExternalDeclaration::new(
                identity.clone(),
                module.clone(),
                NameKey::new("Identity", false).unwrap(),
                DeclarationKind::Trait,
            )
            .unwrap(),
            ExternalDeclaration::new(
                probe.clone(),
                module,
                NameKey::new("probe", false).unwrap(),
                DeclarationKind::Function,
            )
            .unwrap(),
        ],
        [identity.clone(), probe.clone()],
    )
    .unwrap();
    let trusted = TrustedSignatureEnvironment::new([
        TrustedDeclarationSignature::new(
            identity.clone(),
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([]),
                associated_types: Box::new([TrustedAssociatedType {
                    id: item.clone(),
                    name: "Item".into(),
                }]),
            },
        ),
        TrustedDeclarationSignature::new(
            implementation,
            TrustedSignatureKind::Impl {
                negative: false,
                generic_parameters: if generic {
                    Box::new([TrustedGenericParameter {
                        name: "T".into(),
                        bounds: Box::new([]),
                    }])
                } else {
                    Box::new([])
                },
                trait_ref: TrustedTraitExpr {
                    declaration: identity.clone(),
                    arguments: Box::new([]),
                },
                target,
                associated_bindings: Box::new([TrustedAssociatedBinding {
                    declaration: ExternalDefId::new("std::identity::impl::Item").unwrap(),
                    associated: item.clone(),
                    value,
                }]),
            },
        ),
        TrustedDeclarationSignature::new(
            probe.clone(),
            TrustedSignatureKind::Callable {
                bang: false,
                generic_parameters: Box::new([]),
                parameters: Box::new([]),
                result: TrustedTypeExpr::Intrinsic {
                    head: IntrinsicNominal::Opt,
                    arguments: Box::new([TrustedTypeExpr::AssociatedProjection {
                        base: Box::new(base),
                        trait_declaration: identity,
                        trait_arguments: Box::new([]),
                        associated: item,
                    }]),
                },
                requirements: Box::new([]),
            },
        ),
    ])
    .unwrap();
    let source = "fn local() -> Unit = ()\n";
    let mut support = CapabilitySet::signatures();
    support.0.insert(CompileCapability::Typecheck(
        TypecheckCapability::ResolveTraitEnvironment,
    ));
    let mut analysis = analyze(source, &support, Some(&external), Some(&trusted));
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
    let signatures = analysis.signatures.as_mut().unwrap();
    let PhaseProduct::Performed(environment) = eglur_sema::build_trait_environment(
        &analysis.declarations,
        &sources,
        &support,
        signatures,
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("trait environment was not performed")
    };
    let FactState::Known(DeclarationSignature::Callable(callable)) =
        signatures.declaration(&CanonicalTarget::External(probe))
    else {
        panic!("missing probe")
    };
    let result = signatures.callable(*callable).unwrap().result();
    let TypeKind::Nominal { substitution, .. } = signatures.types().kind(result).unwrap() else {
        panic!("missing wrapper")
    };
    let projected = signatures.types().substitution(*substitution).unwrap()[0];
    let TypeKind::AssociatedProjection(projection) = signatures.types().kind(projected).unwrap()
    else {
        panic!("missing projection")
    };
    let obligation = TraitObligation::new(projection.base(), projection.trait_ref().clone());
    (analysis, environment, result, obligation)
}

fn borrowed(mutability: ReferenceMutability, referent: TrustedTypeExpr) -> TrustedTypeExpr {
    TrustedTypeExpr::Reference {
        mutability,
        referent: Box::new(referent),
    }
}

#[test]
fn blanket_impl_cannot_hide_reference_arguments_in_a_projection() {
    let integer = TrustedTypeExpr::Primitive(PrimitiveType::I64);
    let shared = borrowed(ReferenceMutability::Shared, integer.clone());
    for subject in [
        shared.clone(),
        borrowed(ReferenceMutability::Mutable, integer.clone()),
        TrustedTypeExpr::Tuple(Box::new([shared.clone(), integer.clone()])),
        TrustedTypeExpr::Callable {
            parameters: Box::new([shared.clone()]),
            result: Box::new(integer.clone()),
        },
        TrustedTypeExpr::Callable {
            parameters: Box::new([integer]),
            result: Box::new(shared),
        },
    ] {
        let (mut analysis, mut environment, result, obligation) = environment(
            TrustedTypeExpr::GenericParameter(0),
            TrustedTypeExpr::GenericParameter(0),
            true,
            subject,
        );
        let owner = source_def(&analysis.declarations, "local", DeclarationKind::Function);
        let signatures = analysis.signatures.as_mut().unwrap();
        for _ in 0..2 {
            let normalized = environment
                .normalize_for_owner(owner, signatures, result)
                .unwrap();
            assert!(
                matches!(
                    normalized,
                    TraitNormalization::InvalidArgument(
                        eglur_sema::ImplArgumentViolation::Reference(_)
                    )
                ),
                "forbidden impl argument normalized successfully: {normalized:?}"
            );
            let proof = environment
                .prove_for_owner(owner, signatures, obligation.clone())
                .unwrap();
            assert!(
                matches!(
                    proof,
                    TraitProof::InvalidArgument(eglur_sema::ImplArgumentViolation::Reference(_))
                ),
                "forbidden impl argument produced evidence: {proof:?}"
            );
        }
    }
}

#[test]
fn reference_impl_targets_preserve_explicit_borrows_outside_parameters() {
    for generic in [false, true] {
        let integer = TrustedTypeExpr::Primitive(PrimitiveType::I64);
        let parameter = if generic {
            TrustedTypeExpr::GenericParameter(0)
        } else {
            integer.clone()
        };
        let (mut analysis, mut environment, result, obligation) = environment(
            borrowed(ReferenceMutability::Shared, parameter.clone()),
            parameter,
            generic,
            borrowed(ReferenceMutability::Shared, integer),
        );
        let owner = source_def(&analysis.declarations, "local", DeclarationKind::Function);
        let signatures = analysis.signatures.as_mut().unwrap();
        let TraitNormalization::Normalized(ty) = environment
            .normalize_for_owner(owner, signatures, result)
            .unwrap()
        else {
            panic!("explicit borrow did not normalize")
        };
        assert!(!signatures.types().contains_reference(ty).unwrap());
        assert!(matches!(
            environment
                .prove_for_owner(owner, signatures, obligation)
                .unwrap(),
            TraitProof::Implementation(_)
        ));
    }
}
