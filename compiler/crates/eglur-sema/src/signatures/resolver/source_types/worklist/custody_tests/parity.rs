use super::*;

fn fingerprints(
    resolver: &SignatureResolver<'_, '_>,
    sources: &[SemanticSource<'_>],
    mut semantics: ResolvedDeclarationSemantics,
    diagnostics: &mut DiagnosticStore,
) -> crate::SemanticFingerprints {
    fn performed<T>(phase: PhaseAnalysis<T>) -> T {
        let PhaseProduct::Performed(value) = phase.into_parts().0 else {
            panic!("incomplete phase")
        };
        value
    }
    let collection = resolver.collection;
    let imports = resolver.imports;
    let names = performed(
        crate::resolve_names(
            collection,
            imports,
            sources,
            &Support,
            resolver.external_names,
            diagnostics,
        )
        .unwrap(),
    );
    let mut traits = performed(
        crate::build_trait_environment(
            collection,
            sources,
            &Support,
            semantics.signatures_mut(),
            diagnostics,
        )
        .unwrap(),
    );
    let bodies = performed(
        crate::typecheck_bodies(
            collection,
            imports,
            &names,
            resolver.external_names,
            sources,
            &Support,
            &mut semantics,
            &mut traits,
            diagnostics,
        )
        .unwrap(),
    );
    let evidence = performed(
        crate::resolve_trait_evidence(
            collection,
            sources,
            &Support,
            semantics.signatures_mut(),
            &mut traits,
            &bodies,
            diagnostics,
        )
        .unwrap(),
    );
    let effects = crate::analyze_effects(
        collection,
        &names,
        sources,
        &Support,
        semantics.signatures(),
        &bodies,
        &evidence,
        None,
        diagnostics,
    )
    .unwrap()
    .into_parts()
    .0;
    crate::SemanticFingerprints::compute(
        collection,
        imports,
        &names,
        semantics.signatures(),
        sources,
        &bodies,
        &traits,
        &PhaseProduct::Performed(evidence),
        &effects,
    )
    .unwrap()
    .unwrap()
}

#[test]
fn completed_full_semantic_fingerprints_ignore_custody_and_allowance_values() {
    with_resolver(
        &[
            "pub type Pair[T] = (T, T)\npub fn inspect(x: Pair[i32]) -> Pair[i32] = x\npub fn relay(x: (i32, i32)) -> (i32, i32) = inspect(x)",
        ],
        |resolver, sources| {
            let mut diagnostics = DiagnosticStore::new();
            let PhaseProduct::Performed(semantics) = resolve_declaration_signatures(
                resolver.collection,
                resolver.imports,
                sources,
                &Support,
                SignatureResolutionInputs::new(resolver.external_names, None),
                &mut diagnostics,
            )
            .unwrap()
            .into_parts()
            .0
            else {
                panic!("unmetered phase incomplete")
            };
            let expected = fingerprints(resolver, sources, semantics, &mut diagnostics);
            let expected_diagnostics = diagnostics.finalize(None);
            for allowance in [ALLOWANCE, ALLOWANCE * 2] {
                let mut work = accounting(1);
                let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
                let mut diagnostics = DiagnosticStore::new();
                let semantics = completed(
                    run(
                        resolver,
                        sources,
                        &mut diagnostics,
                        &mut work,
                        &mut inputs,
                        &|_| allowance,
                    )
                    .unwrap(),
                );
                assert_eq!(
                    fingerprints(resolver, sources, semantics, &mut diagnostics),
                    expected
                );
                assert_eq!(diagnostics.finalize(None), expected_diagnostics);
            }
        },
    );
}
