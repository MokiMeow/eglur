use super::*;
use crate::{
    CompileCapability, CompileSupport, DeclarationCollection, DeclarationSignature,
    ImportAnalysisUnit, ModuleAddress, PhaseProduct, PrimitiveType, SemanticSource,
    SignatureResolutionInputs, collect_declarations, plan_imports, resolve_declaration_signatures,
    resolve_imports,
};
use eglur_diagnostics::{DiagnosticStore, SourceLabel, SourceSpan};
use eglur_hir::lower;
use eglur_source::{FileId, LineIndex, SourceFile};
use eglur_syntax::parse_validated;

struct AllCapabilities;

impl CompileSupport for AllCapabilities {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

struct Fixture {
    declarations: DeclarationCollection,
    signatures: DeclarationSignatures,
    environment: TraitEnvironment,
    diagnostics: DiagnosticStore,
    span: SourceSpan,
}

impl Fixture {
    fn declaration(&self, name: &str) -> DefId {
        self.declarations
            .modules()
            .flat_map(|module| module.declarations())
            .find(|declaration| {
                declaration
                    .name()
                    .is_some_and(|candidate| candidate.spelling() == name)
            })
            .unwrap()
            .id()
    }
}

fn fixture() -> Fixture {
    let text = concat!(
        "trait Link[A] {}\ntrait Carries { type Item }\nstruct Carrier {}\n",
        "impl Carries for Carrier { type Item = i64 }\n",
        "type Item[T: Carries] = T::Item\n",
        "fn take[T: Link[Item[Carrier]]](x: T) -> Item[Carrier] = 1\n",
        "fn independent[T: Link[Bool]](x: T) -> Bool = true\n",
        "fn direct() -> Unit = ()\n",
    );
    let file = FileId::new(0);
    let input = SourceFile::new(file, text.as_bytes().to_vec());
    let validated = input.validate().unwrap();
    let lines = LineIndex::new(validated);
    let parse = parse_validated(validated);
    assert!(parse.diagnostics().is_empty());
    let hir = lower(&parse);
    assert!(hir.invalid_occurrences().is_empty());
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
    let span = spans[0].1.clone();
    let sources = [SemanticSource::new(
        file,
        ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
        Some(&hir),
        spans,
        [],
        None,
    )
    .unwrap()];
    let mut diagnostics = DiagnosticStore::new();
    let PhaseProduct::Performed(declarations) = collect_declarations(&sources, &AllCapabilities)
        .unwrap()
        .into_parts()
        .0
    else {
        panic!("declarations unavailable")
    };
    let PhaseProduct::Performed(plan) = plan_imports(
        &declarations,
        &sources,
        &AllCapabilities,
        None,
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("import plan unavailable")
    };
    let PhaseProduct::Performed(imports) = resolve_imports(
        &declarations,
        &plan,
        &[ImportAnalysisUnit::new([file]).unwrap()],
        &sources,
        &AllCapabilities,
        None,
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("imports unavailable")
    };
    let PhaseProduct::Performed(resolved) = resolve_declaration_signatures(
        &declarations,
        &imports,
        &sources,
        &AllCapabilities,
        SignatureResolutionInputs::new(None, None),
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("signatures unavailable")
    };
    let (mut signatures, _) = resolved.into_parts();
    let PhaseProduct::Performed(environment) = crate::build_trait_environment(
        &declarations,
        &sources,
        &AllCapabilities,
        &mut signatures,
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("trait environment unavailable")
    };
    Fixture {
        declarations,
        signatures,
        environment,
        diagnostics,
        span,
    }
}

#[test]
fn poisoning_an_impl_revalidates_cached_owner_proofs_and_projections() {
    let mut fixture = fixture();
    let owner = fixture.declaration("take");
    let independent = fixture.declaration("independent");
    let direct = fixture.declaration("direct");
    let carrier_trait = CanonicalTarget::Source(fixture.declaration("Carries"));
    let declared = fixture.environment.assumptions[&owner][0].clone();
    let independent_bound = fixture.environment.assumptions[&independent][0].clone();
    let integer = fixture
        .signatures
        .types_mut()
        .primitive(PrimitiveType::I64)
        .unwrap();
    let arguments = fixture
        .signatures
        .types_mut()
        .intern_substitution([integer])
        .unwrap();
    let required = TraitObligation::new(
        declared.obligation.subject(),
        TraitRef::new(
            declared.obligation.trait_ref().declaration().clone(),
            arguments,
        ),
    );
    let implementation = fixture
        .environment
        .implementations
        .iter()
        .find(|(_, implementation)| {
            implementation.header.trait_ref().declaration() == &carrier_trait
        })
        .map(|(target, _)| target.clone())
        .unwrap();
    let header = fixture.environment.implementations[&implementation]
        .header
        .clone();
    let concrete = TraitObligation::new(header.target(), header.trait_ref().clone());
    let FactState::Known(DeclarationSignature::Callable(callable)) =
        fixture.signatures.source_declaration(owner)
    else {
        panic!("missing callable")
    };
    let projection = fixture.signatures.callable(*callable).unwrap().result();
    let TraitAssumptionEvidence::Witness(witness) = declared.evidence else {
        panic!("missing witness")
    };

    for _ in 0..2 {
        assert!(
            matches!(fixture.environment.prove_for_owner(owner, &mut fixture.signatures, required.clone()).unwrap(), TraitProof::Bound(found) if found == witness)
        );
        assert!(
            matches!(fixture.environment.normalize_for_owner(owner, &mut fixture.signatures, projection).unwrap(), TraitNormalization::Normalized(found) if found == integer)
        );
        assert!(matches!(
            fixture
                .environment
                .prove_for_owner(direct, &mut fixture.signatures, concrete.clone())
                .unwrap(),
            TraitProof::Implementation(_)
        ));
        assert!(matches!(
            fixture
                .environment
                .prove_for_owner(
                    independent,
                    &mut fixture.signatures,
                    independent_bound.obligation.clone()
                )
                .unwrap(),
            TraitProof::Bound(_)
        ));
    }
    assert_eq!(
        fixture.environment.witness_obligation(witness),
        Some(&declared.obligation)
    );

    let mut fresh = TraitEnvironment::from_checked_parts(
        fixture.environment.implementations.clone(),
        fixture.environment.buckets.clone(),
        fixture.environment.assumptions.clone(),
    );
    let poison = crate::diagnostic::record_name_not_found(
        &mut fixture.diagnostics,
        fixture.span,
        "invalidated_impl",
    )
    .unwrap();
    fixture
        .environment
        .poison_implementation(&implementation, poison.clone());
    fresh.poison_implementation(&implementation, poison);

    for environment in [&mut fixture.environment, &mut fresh] {
        assert!(matches!(
            environment
                .prove_for_owner(owner, &mut fixture.signatures, required.clone())
                .unwrap(),
            TraitProof::Poisoned(_)
        ));
        assert!(matches!(
            environment
                .normalize_for_owner(owner, &mut fixture.signatures, projection)
                .unwrap(),
            TraitNormalization::Poisoned(_)
        ));
        assert!(matches!(
            environment
                .prove_for_owner(direct, &mut fixture.signatures, concrete.clone())
                .unwrap(),
            TraitProof::Poisoned(_)
        ));
        assert!(matches!(
            environment
                .prove_for_owner(
                    independent,
                    &mut fixture.signatures,
                    independent_bound.obligation.clone()
                )
                .unwrap(),
            TraitProof::Bound(_)
        ));
        assert_eq!(
            environment.witness_obligation(witness),
            Some(&declared.obligation)
        );
    }
    assert_eq!(fixture.diagnostics.finalize(None).diagnostics().len(), 1);
}
