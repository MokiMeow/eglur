use super::*;
use crate::{
    SemanticFingerprints, analyze_effects, analyze_ownership, build_trait_environment,
    resolve_names, resolve_trait_evidence, typecheck_bodies,
};

struct FingerprintSupport;

thread_local! {
    static SIGNATURE_ROLE_WORK: Cell<RoleWork> = const { Cell::new(RoleWork {
        insertions: 0, queries: 0, generic_index_insertions: 0, generic_index_queries: 0,
    }) };
    static SIGNATURE_DECLARATION_VISITS: Cell<usize> = const { Cell::new(0) };
}

impl CompileSupport for FingerprintSupport {
    fn supports(&self, capability: CompileCapability) -> bool {
        matches!(
            capability,
            CompileCapability::Resolve(
                ResolveCapability::CollectDeclarations
                    | ResolveCapability::ResolveFileSetImports
                    | ResolveCapability::ResolveLexicalValues
                    | ResolveCapability::ResolveModuleValues
                    | ResolveCapability::ResolveQualifiedValues
                    | ResolveCapability::ResolvePatternScopes
                    | ResolveCapability::ResolveTrustedExternalNames
            ) | CompileCapability::Typecheck(
                TypecheckCapability::ResolveDeclarationSignatures
                    | TypecheckCapability::ResolveTraitEnvironment
                    | TypecheckCapability::CheckBodies
                    | TypecheckCapability::ResolveMembers
                    | TypecheckCapability::ResolveMethods
                    | TypecheckCapability::ResolveOverloads
                    | TypecheckCapability::AnalyzeInhabitance
                    | TypecheckCapability::CheckFailureFlow
                    | TypecheckCapability::CheckSelectedCheckedIntegerArithmetic
                    | TypecheckCapability::CheckBranching
                    | TypecheckCapability::ResolveTraitEvidence
                    | TypecheckCapability::ComputeEffectSummaries
                    | TypecheckCapability::CheckOwnershipFlow
            )
        )
    }
}

fn fingerprint(texts: &[(&str, &str)], expected_edges: usize) -> SemanticFingerprints {
    with_sources(texts, |sources| {
        let support = FingerprintSupport;
        let external = ExternalNameEnvironment::new([], [], []).unwrap();
        let mut diagnostics = DiagnosticStore::new();
        CHILD_WORK.set(ChildWork::default());
        let declarations = performed(collect_declarations(sources, &support).unwrap());
        assert_eq!(CHILD_WORK.get().insertions, expected_edges);
        assert_eq!(
            declarations
                .modules()
                .map(|module| module
                    .children_by_parent
                    .values()
                    .map(Vec::len)
                    .sum::<usize>())
                .sum::<usize>(),
            expected_edges
        );
        let plan = performed(
            plan_imports(
                &declarations,
                sources,
                &support,
                Some(&external),
                &mut diagnostics,
            )
            .unwrap(),
        );
        let units = [ImportAnalysisUnit::new(sources.iter().map(SemanticSource::file)).unwrap()];
        let imports = performed(
            resolve_imports(
                &declarations,
                &plan,
                &units,
                sources,
                &support,
                Some(&external),
                &mut diagnostics,
            )
            .unwrap(),
        );
        let names = performed(
            resolve_names(
                &declarations,
                &imports,
                sources,
                &support,
                Some(&external),
                &mut diagnostics,
            )
            .unwrap(),
        );
        ROLE_WORK.set(RoleWork::default());
        DECLARATION_VISITS.set(0);
        let mut semantics = performed(
            resolve_declaration_signatures(
                &declarations,
                &imports,
                sources,
                &support,
                SignatureResolutionInputs::new(Some(&external), None),
                &mut diagnostics,
            )
            .unwrap(),
        );
        SIGNATURE_ROLE_WORK.set(ROLE_WORK.get());
        SIGNATURE_DECLARATION_VISITS.set(DECLARATION_VISITS.get());
        let mut traits = performed(
            build_trait_environment(
                &declarations,
                sources,
                &support,
                semantics.signatures_mut(),
                &mut diagnostics,
            )
            .unwrap(),
        );
        let bodies = performed(
            typecheck_bodies(
                &declarations,
                &imports,
                &names,
                Some(&external),
                sources,
                &support,
                &mut semantics,
                &mut traits,
                &mut diagnostics,
            )
            .unwrap(),
        );
        let ownership = analyze_ownership(
            &declarations,
            &names,
            sources,
            &support,
            semantics.signatures(),
            &bodies,
            &mut diagnostics,
        )
        .unwrap()
        .into_parts()
        .0;
        assert!(matches!(ownership, PhaseProduct::Performed(_)));
        let evidence = performed(
            resolve_trait_evidence(
                &declarations,
                sources,
                &support,
                semantics.signatures_mut(),
                &mut traits,
                &bodies,
                &mut diagnostics,
            )
            .unwrap(),
        );
        let effects = analyze_effects(
            &declarations,
            &names,
            sources,
            &support,
            semantics.signatures(),
            &bodies,
            &evidence,
            None,
            &mut diagnostics,
        )
        .unwrap()
        .into_parts()
        .0;
        assert!(matches!(effects, PhaseProduct::Performed(_)));
        assert!(diagnostics.finalize(None).diagnostics().is_empty());

        CHILD_WORK.set(ChildWork::default());
        SemanticFingerprints::compute(
            &declarations,
            &imports,
            &names,
            semantics.signatures(),
            sources,
            &bodies,
            &traits,
            &PhaseProduct::Performed(evidence),
            &effects,
        )
        .unwrap()
        .expect("complete semantic products produce fingerprints")
    })
}

#[test]
fn wide_signature_resolution_borrows_children_and_looks_up_generic_slots() {
    for count in [8_usize, 32, 128] {
        let parameters = (0..count)
            .map(|index| format!("T{index:03}"))
            .collect::<Vec<_>>()
            .join(", ");
        for public in [false, true] {
            let visibility = if public { "pub " } else { "" };
            let text = format!("{visibility}fn wide[{parameters}](value: Bool) -> Bool = value\n");
            let _ = fingerprint(&[("main", &text)], count);
            assert_eq!(
                SIGNATURE_ROLE_WORK.get(),
                RoleWork {
                    queries: 2,
                    generic_index_queries: 2 * count,
                    ..RoleWork::default()
                }
            );
            assert!(
                SIGNATURE_DECLARATION_VISITS.get() <= 32 * (count + 1),
                "G={count}, visits={}",
                SIGNATURE_DECLARATION_VISITS.get()
            );
        }
    }
}

#[test]
fn reexported_fingerprints_visit_only_direct_family_children() {
    for count in [8_usize, 32, 128] {
        let methods = (0..count)
            .map(|index| {
                format!("pub fn method_{index:03}[T](self: &Self, value: T) -> T = value\n")
            })
            .collect::<String>();
        let unrelated = (0..count)
            .map(|index| format!("fn unrelated_{index:03}(value: Bool) -> Bool = value\n"))
            .collect::<String>();
        let source = format!(
            "pub struct Host[A] {{\n{methods}\
             fn hidden[H](self: &Self, value: H) -> H = value\n}}\n{unrelated}"
        );
        for (import, public_copies) in [
            ("import dep::Host\n", 0),
            ("pub import dep::Host\n", 1),
            (
                "pub import dep::Host as Left\npub import dep::Host as Right\n",
                2,
            ),
        ] {
            let mut entries = [("dep", source.as_str()), ("main", import)];
            let expected = ChildWork {
                insertions: 0,
                queries: 2 * public_copies * (2 * count + 2),
                visits: 2 * public_copies * (2 * count + 2),
            };
            let canonical = fingerprint(&entries, 2 * count + 3);
            assert_eq!(CHILD_WORK.get(), expected);
            entries.reverse();
            assert_eq!(fingerprint(&entries, 2 * count + 3), canonical);
            assert_eq!(CHILD_WORK.get(), expected);
        }

        let chain = [
            ("dep", source.as_str()),
            ("bridge", "pub import dep::Host\n"),
            ("main", "pub import bridge::Host\n"),
        ];
        let _ = fingerprint(&chain, 2 * count + 3);
        assert_eq!(
            CHILD_WORK.get(),
            ChildWork {
                insertions: 0,
                queries: 4 * (2 * count + 2),
                visits: 4 * (2 * count + 2),
            }
        );
    }
}
