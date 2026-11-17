use super::*;
use eglur_diagnostics::{DiagnosticStore, SourceLabel, SourceSpan};
use eglur_hir::lower;
use eglur_sema::{
    CompileCapability, CompileSupport, DeclarationCollection, DeclarationSignatures, DefId,
    FactState, ImportAnalysisUnit, ModuleAddress, PhaseAnalysis, PhaseProduct, PrimitiveType,
    SemanticSource, SignatureResolutionInputs, TraitEnvironment, TraitObligation, TraitProof,
    TypeKind, build_trait_environment, collect_declarations, plan_imports,
    resolve_declaration_signatures, resolve_imports, resolve_names, resolve_trait_evidence,
    typecheck_bodies,
};
use eglur_source::{FileId, LineIndex, SourceFile};
use eglur_syntax::parse_validated;

struct AllCapabilities;

impl CompileSupport for AllCapabilities {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

fn performed<T>(phase: PhaseAnalysis<T>) -> T {
    let PhaseProduct::Performed(product) = phase.into_parts().0 else {
        panic!("native fixture phase was not performed")
    };
    product
}

pub(super) fn declaration(collection: &DeclarationCollection, name: &str) -> DefId {
    collection
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

pub(super) fn with_evidence<R>(
    text: &str,
    owner_name: &str,
    warm: impl FnOnce(&DeclarationCollection, &mut DeclarationSignatures, &mut TraitEnvironment, DefId),
    inspect: impl FnOnce(&TraitEvidence, &DeclarationCollection) -> R,
) -> R {
    let file = FileId::new(0);
    let input = SourceFile::new(file, text.as_bytes().to_vec());
    let validated = input.validate().unwrap();
    let lines = LineIndex::new(validated);
    let parsed = parse_validated(validated);
    assert!(parsed.diagnostics().is_empty());
    let hir = lower(&parsed);
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
    let sources = [SemanticSource::new(
        file,
        ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
        Some(&hir),
        spans,
        [],
        None,
    )
    .unwrap()];
    let support = AllCapabilities;
    let mut diagnostics = DiagnosticStore::new();
    let declarations = performed(collect_declarations(&sources, &support).unwrap());
    let plan =
        performed(plan_imports(&declarations, &sources, &support, None, &mut diagnostics).unwrap());
    let imports = performed(
        resolve_imports(
            &declarations,
            &plan,
            &[ImportAnalysisUnit::new([file]).unwrap()],
            &sources,
            &support,
            None,
            &mut diagnostics,
        )
        .unwrap(),
    );
    let names = performed(
        resolve_names(
            &declarations,
            &imports,
            &sources,
            &support,
            None,
            &mut diagnostics,
        )
        .unwrap(),
    );
    let mut semantics = performed(
        resolve_declaration_signatures(
            &declarations,
            &imports,
            &sources,
            &support,
            SignatureResolutionInputs::new(None, None),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let mut environment = performed(
        build_trait_environment(
            &declarations,
            &sources,
            &support,
            semantics.signatures_mut(),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let owner = declaration(&declarations, owner_name);
    warm(
        &declarations,
        semantics.signatures_mut(),
        &mut environment,
        owner,
    );
    let bodies = performed(
        typecheck_bodies(
            &declarations,
            &imports,
            &names,
            None,
            &sources,
            &support,
            &mut semantics,
            &mut environment,
            &mut diagnostics,
        )
        .unwrap(),
    );
    let evidence = performed(
        resolve_trait_evidence(
            &declarations,
            &sources,
            &support,
            semantics.signatures_mut(),
            &mut environment,
            &bodies,
            &mut diagnostics,
        )
        .unwrap(),
    );
    assert!(
        !diagnostics.has_errors(),
        "source fixture produced semantic errors"
    );
    assert_eq!(evidence.invocations().len(), bodies.invocations().len());
    assert!(
        evidence
            .invocations()
            .all(|(_, fact)| matches!(fact, FactState::Known(_)))
    );
    inspect(&evidence, &declarations)
}

pub(super) fn warm_chain(
    declarations: &DeclarationCollection,
    signatures: &mut DeclarationSignatures,
    environment: &mut TraitEnvironment,
    owner: DefId,
    depth: usize,
) {
    let targets = declarations
        .modules()
        .flat_map(|module| module.declarations())
        .filter_map(|declaration| {
            declaration
                .name()
                .map(|name| (name.spelling().to_owned(), declaration.id()))
        })
        .collect::<BTreeMap<_, _>>();
    let integer = signatures
        .types()
        .types()
        .find_map(|(id, kind)| {
            matches!(kind, TypeKind::Primitive(PrimitiveType::I64)).then_some(id)
        })
        .unwrap();
    let trait_refs = declarations
        .modules()
        .flat_map(|module| module.declarations())
        .filter_map(|declaration| {
            let target = CanonicalTarget::Source(declaration.id());
            let FactState::Known(header) = signatures.impl_header(&target) else {
                return None;
            };
            Some((
                header.trait_ref().declaration().clone(),
                header.trait_ref().clone(),
            ))
        })
        .collect::<BTreeMap<_, _>>();

    for level in 0..depth {
        let target = CanonicalTarget::Source(targets[&format!("Hop{level}")]);
        let obligation = TraitObligation::new(integer, trait_refs[&target].clone());
        assert!(matches!(
            environment
                .prove_for_owner(owner, signatures, obligation)
                .unwrap(),
            TraitProof::Implementation(_)
        ));
    }
}

pub(super) fn root_proof(
    evidence: &TraitEvidence,
    declarations: &DeclarationCollection,
    trait_name: &str,
) -> EvidenceProofId {
    let target = CanonicalTarget::Source(declaration(declarations, trait_name));
    evidence
        .environments()
        .flat_map(|environment| environment.entries())
        .find(|entry| entry.trait_ref().declaration() == &target)
        .unwrap()
        .proof()
}
