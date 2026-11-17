use eglur_diagnostics::{DiagnosticCheckpoint, DiagnosticFailure, DiagnosticSnapshot, SourceLabel};
use eglur_source::{LineIndex, SourceFile};

use super::*;
use crate::{
    ExternalDeclaration, ExternalDefId, ExternalNameEnvironment, ImportAnalysisUnit, ModuleAddress,
    PhaseProduct, PrimitiveType, ReferenceMutability, SignatureResolutionInputs,
    TrustedCallableParameter, TrustedDeclarationInputs, TrustedDeclarationSignature,
    TrustedSignatureEnvironment, TrustedSignatureKind, TrustedTraitMethod, TrustedTypeExpr,
};

struct AllCapabilities;

impl CompileSupport for AllCapabilities {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

fn performed<T>(phase: PhaseAnalysis<T>) -> T {
    let PhaseProduct::Performed(value) = phase.into_parts().0 else {
        panic!("required phase is performed");
    };
    value
}

fn with_methods(
    text: &str,
    external: &ExternalNameEnvironment,
    trusted: Option<&TrustedSignatureEnvironment>,
    inspect: impl FnOnce(
        &DeclarationCollection,
        &mut DeclarationSignatures,
        &TraitEnvironment,
        &mut DiagnosticStore,
        &SemanticSource<'_>,
        &DiagnosticCheckpoint,
    ),
) {
    let file = FileId::new(0);
    let source_file = SourceFile::new(file, text.as_bytes().to_vec());
    let validated = source_file.validate().unwrap();
    let parsed = eglur_syntax::parse_validated(validated);
    assert!(parsed.diagnostics().is_empty());
    let hir = eglur_hir::lower(&parsed);
    assert!(hir.invalid_occurrences().is_empty());
    let lines = LineIndex::new(validated);
    let label = SourceLabel::new("main.eglur").unwrap();
    let spans = hir
        .origins()
        .map(|(origin, value)| {
            (
                origin,
                SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let sources = [SemanticSource::new(
        file,
        ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
        Some(&hir),
        spans,
        [],
        None,
    )
    .unwrap()];
    let declarations = performed(crate::collect_declarations(&sources, &AllCapabilities).unwrap());
    let mut diagnostics = DiagnosticStore::new();
    let plan = performed(
        crate::plan_imports(
            &declarations,
            &sources,
            &AllCapabilities,
            Some(external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let imports = performed(
        crate::resolve_imports(
            &declarations,
            &plan,
            &[ImportAnalysisUnit::new([file]).unwrap()],
            &sources,
            &AllCapabilities,
            Some(external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let resolved = performed(
        crate::resolve_declaration_signatures(
            &declarations,
            &imports,
            &sources,
            &AllCapabilities,
            SignatureResolutionInputs::new(
                Some(external),
                trusted.map(|trusted| TrustedDeclarationInputs::new(trusted, None)),
            ),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let (mut signatures, _) = resolved.into_parts();
    let checkpoint = diagnostics.checkpoint();
    let environment = performed(
        crate::build_trait_environment(
            &declarations,
            &sources,
            &AllCapabilities,
            &mut signatures,
            &mut diagnostics,
        )
        .unwrap(),
    );
    inspect(
        &declarations,
        &mut signatures,
        &environment,
        &mut diagnostics,
        &sources[0],
        &checkpoint,
    );
}

#[test]
fn missing_methods_poison_the_environment_without_publishing_dispatch_targets() {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    for count in [1, 2, 8, 32] {
        let members = (0..count)
            .map(|index| format!("fn required{index}(self: &Self) -> i64\n"))
            .collect::<String>();
        let text = format!(
            "trait Mark {{ {members} fn defaulted(self: &Self) -> i64 = 1 }}\nstruct Host {{}}\nimpl Mark for Host {{}}\n"
        );
        with_methods(
            &text,
            &external,
            None,
            |collection, signatures, environment, diagnostics, source, checkpoint| {
                let inputs = environment.implementation_validation_inputs();
                assert_eq!(inputs.len(), 1);
                let input = &inputs[0];
                let poison = input.existing_poison.as_ref().unwrap();
                let FactState::Poisoned(signature_poison) =
                    signatures.declaration(&input.implementation)
                else {
                    panic!("public contract retains the method failure");
                };
                let FactState::Poisoned(header_poison) =
                    signatures.impl_header(&input.implementation)
                else {
                    panic!("checked impl header retains the method failure");
                };
                assert_eq!(environment.method_targets().count(), 0);
                let CanonicalTarget::Source(owner) = &input.implementation else {
                    panic!("source fixture has a source impl");
                };
                let span = source
                    .origin_span(collection.declaration(*owner).unwrap().origin())
                    .unwrap();
                let all = diagnostics
                    .roots_since_within(checkpoint, span)
                    .unwrap()
                    .unwrap();
                for propagated in [poison, signature_poison, header_poison] {
                    diagnostics
                        .record_failure(DiagnosticFailure::Derivative(propagated.clone()))
                        .unwrap();
                    diagnostics
                        .select_preceding_roots(propagated, &all)
                        .unwrap();
                }
                let snapshot = std::mem::take(diagnostics).finalize(None);
                assert_eq!(snapshot.diagnostics().len(), count);
                assert!(snapshot.diagnostics().iter().all(|diagnostic| diagnostic.code().as_str() == "TRAIT_IMPL_MISSING_METHOD"));
            },
        );
    }
}

#[test]
fn complete_method_contracts_publish_required_and_default_dispatch_targets() {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    with_methods(
        "trait Mark { fn first(self: &Self) -> i64\nfn second(self: &Self) -> i64\nfn defaulted(self: &Self) -> i64 = 7 }\nstruct Host {}\nimpl Mark for Host { fn first(self: &Self) -> i64 = 1\nfn second(self: &Self) -> i64 = 2 }\n",
        &external,
        None,
        |_, signatures, environment, diagnostics, _, _| {
            let inputs = environment.implementation_validation_inputs();
            assert!(inputs[0].existing_poison.is_none());
            assert_eq!(environment.method_targets().count(), 3);
            for method in signatures.trait_methods(inputs[0].header.trait_ref().declaration()) {
                let target = environment
                    .checked_impl_method(&inputs[0].implementation, method.declaration())
                    .unwrap();
                if method.has_default() {
                    assert_eq!(target, method.declaration());
                } else {
                    assert!(matches!(target, CanonicalTarget::Source(_)));
                }
            }
            assert!(matches!(
                std::mem::take(diagnostics).finalize(None),
                DiagnosticSnapshot::Accepted(_)
            ));
        },
    );
}

#[test]
fn missing_trusted_trait_methods_use_distinct_external_targets_without_related_source() {
    let module =
        ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("methods")]).unwrap();
    let trait_id = ExternalDefId::new("std::methods::Mark").unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [ExternalDeclaration::new(
            trait_id.clone(),
            module,
            NameKey::new("Mark", false).unwrap(),
            DeclarationKind::Trait,
        )
        .unwrap()],
        [trait_id.clone()],
    )
    .unwrap();
    for count in [1, 2, 8] {
        let methods = (0..count)
            .map(|index| TrustedTraitMethod {
                declaration: ExternalDefId::new(format!("std::methods::Mark::required{index}"))
                    .unwrap(),
                name: NameKey::new(format!("required{index}"), false).unwrap(),
                has_default: false,
            })
            .collect::<Vec<_>>();
        let trait_declaration = TrustedDeclarationSignature::new(
            trait_id.clone(),
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([]),
                associated_types: Box::new([]),
            },
        )
        .with_trait_methods(methods.clone());
        let callables = methods.iter().map(|method| {
            TrustedDeclarationSignature::new(
                method.declaration.clone(),
                TrustedSignatureKind::Callable {
                    bang: false,
                    generic_parameters: Box::new([]),
                    parameters: Box::new([TrustedCallableParameter {
                        name: Some("self".into()),
                        ty: TrustedTypeExpr::Reference {
                            mutability: ReferenceMutability::Shared,
                            referent: Box::new(TrustedTypeExpr::SelfType(trait_id.clone())),
                        },
                    }]),
                    result: TrustedTypeExpr::Primitive(PrimitiveType::I64),
                    requirements: Box::new([]),
                },
            )
        });
        let trusted =
            TrustedSignatureEnvironment::new(std::iter::once(trait_declaration).chain(callables))
                .unwrap();
        with_methods(
            "struct Host {}\nimpl Mark for Host {}\n",
            &external,
            Some(&trusted),
            |_, _, environment, diagnostics, _, _| {
                assert!(
                    environment.implementation_validation_inputs()[0]
                        .existing_poison
                        .is_some()
                );
                assert_eq!(environment.method_targets().count(), 0);
                let snapshot = std::mem::take(diagnostics).finalize(None);
                assert_eq!(snapshot.diagnostics().len(), count);
                for diagnostic in snapshot.diagnostics() {
                    assert_eq!(diagnostic.code().as_str(), "TRAIT_IMPL_MISSING_METHOD");
                    assert_eq!(
                        diagnostic.data().get("trait"),
                        Some(&DiagnosticDataValue::from("Mark"))
                    );
                    assert!(diagnostic.related().is_empty());
                }
            },
        );
    }
}
