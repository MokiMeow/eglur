use std::collections::BTreeMap;

use super::*;

fn analyze(text: &str) -> DiagnosticSnapshot {
    let mut session = session(text);
    assert!(session.sources()[0].parse().unwrap().is_success());
    let catalog = TrustedCatalog::first_slice().unwrap();
    session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    session.into_diagnostics(None)
}

#[test]
fn multiple_required_method_omissions_keep_impl_blame_and_each_declaration_context() {
    for count in [2, 8, 32] {
        let members = (0..count)
            .map(|index| format!("fn required{index}(self: &Self) -> i64\n"))
            .collect::<String>();
        let text = format!(
            "trait Mark {{ {members} }}\nstruct Host {{}}\nimpl Mark for Host {{}}\nfn after() -> i64 = 1\n"
        );
        let mut session = session(&text);
        assert!(session.sources()[0].parse().unwrap().is_success());
        let origins = session.sources()[0]
            .hir()
            .unwrap()
            .origins()
            .map(|(origin, value)| (origin, value.source()))
            .collect::<BTreeMap<_, _>>();
        let catalog = TrustedCatalog::first_slice().unwrap();
        let analysis = session.resolve(&trait_profile(), Some(&catalog)).unwrap();
        let resolution = analysis.resolution().unwrap();
        let module = resolution.declarations().modules().next().unwrap();
        let trait_id = module
            .declarations()
            .find(|declaration| {
                declaration.kind() == DeclarationKind::Trait
                    && declaration.name().is_some_and(|name| name.text() == "Mark")
            })
            .unwrap()
            .id();
        let expected_related = module
            .declarations()
            .filter(|declaration| {
                declaration.parent() == Some(trait_id)
                    && declaration.kind() == DeclarationKind::Function
            })
            .map(|declaration| {
                (
                    declaration.name().unwrap().spelling(),
                    origins[&declaration.origin()],
                )
            })
            .collect::<BTreeMap<_, _>>();
        let after = module
            .declarations()
            .find(|declaration| {
                declaration.kind() == DeclarationKind::Function
                    && declaration
                        .name()
                        .is_some_and(|name| name.text() == "after")
            })
            .unwrap();
        assert!(matches!(
            resolution.signatures().source_declaration(after.id()),
            FactState::Known(_)
        ));

        let snapshot = session.into_diagnostics(None);
        assert!(matches!(snapshot, DiagnosticSnapshot::Rejected(_)));
        let diagnostics = snapshot.diagnostics();
        assert_eq!(diagnostics.len(), count);
        let primary = diagnostics[0].primary_span().as_source().unwrap();
        let range = primary.file_span().range();
        assert_eq!(
            &text[range.start().get() as usize..range.end().get() as usize],
            "impl Mark for Host {}"
        );
        let mut found = BTreeMap::new();
        for diagnostic in diagnostics {
            assert_eq!(diagnostic.code().as_str(), "TRAIT_IMPL_MISSING_METHOD");
            assert_eq!(diagnostic.primary_span().as_source().unwrap(), primary);
            assert_eq!(
                diagnostic.data().get("trait"),
                Some(&DiagnosticDataValue::from("Mark"))
            );
            let DiagnosticDataValue::String(method) = diagnostic.data().get("method").unwrap()
            else {
                panic!("method data is text");
            };
            assert_eq!(diagnostic.related().len(), 1);
            assert_eq!(
                diagnostic.related()[0].label(),
                "trait method declared here"
            );
            assert!(
                found
                    .insert(
                        method.to_string(),
                        diagnostic.related()[0].span().file_span()
                    )
                    .is_none()
            );
        }
        assert_eq!(found, expected_related);
    }
}

#[test]
fn missing_method_roots_do_not_count_defaults_or_present_methods() {
    let snapshot = analyze(concat!(
        "trait Mark {\n",
        "fn first(self: &Self) -> i64\n",
        "fn second(self: &Self) -> i64\n",
        "fn present(self: &Self) -> i64\n",
        "fn defaulted(self: &Self) -> i64 = 7\n",
        "}\nstruct Host {}\n",
        "impl Mark for Host { fn present(self: &Self) -> i64 = 1 }\n",
    ));
    let mut methods = snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic.code().as_str(), "TRAIT_IMPL_MISSING_METHOD");
            diagnostic.data().get("method").unwrap().clone()
        })
        .collect::<Vec<_>>();
    methods.sort();
    assert_eq!(
        methods,
        [
            DiagnosticDataValue::from("first"),
            DiagnosticDataValue::from("second")
        ]
    );

    let complete = analyze(concat!(
        "trait Mark { fn first(self: &Self) -> i64\nfn second(self: &Self) -> i64\nfn defaulted(self: &Self) -> i64 = 7 }\n",
        "struct Host {}\nimpl Mark for Host { fn first(self: &Self) -> i64 = 1\nfn second(self: &Self) -> i64 = 2 }\n",
    ));
    assert!(matches!(complete, DiagnosticSnapshot::Accepted(_)));
}

#[test]
fn overloaded_required_methods_keep_distinct_target_identity() {
    let snapshot = analyze(concat!(
        "trait Mark {\n",
        "overload fn read(self: &Self) -> i64\n",
        "overload fn read(self: &Self, value: i64) -> i64\n",
        "}\nstruct Host {}\nimpl Mark for Host {}\n",
    ));
    let diagnostics = snapshot.diagnostics();
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].code().as_str(), "TRAIT_IMPL_MISSING_METHOD");
    assert_eq!(diagnostics[1].code(), diagnostics[0].code());
    assert_eq!(diagnostics[0].data(), diagnostics[1].data());
    assert_eq!(diagnostics[0].message(), diagnostics[1].message());
    assert_eq!(diagnostics[0].primary_span(), diagnostics[1].primary_span());
    assert_ne!(
        diagnostics[0].related()[0].span(),
        diagnostics[1].related()[0].span()
    );
}

#[test]
fn independent_missing_and_mismatched_method_obligations_are_retained() {
    let snapshot = analyze(concat!(
        "trait Mark { fn first(self: &Self) -> i64\nfn second(self: &Self) -> i64\nfn wrong(self: &Self) -> i64 }\n",
        "struct Host {}\nimpl Mark for Host { fn wrong(self: &Self) -> Bool = true }\n",
    ));
    let mut codes = snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect::<Vec<_>>();
    codes.sort_unstable();
    assert_eq!(
        codes,
        [
            "TRAIT_IMPL_MISSING_METHOD",
            "TRAIT_IMPL_MISSING_METHOD",
            "TRAIT_IMPL_SIGNATURE_MISMATCH"
        ]
    );
}

#[test]
fn poisoned_associated_prerequisites_skip_method_obligation_discovery() {
    let snapshot = analyze(concat!(
        "trait Mark { type A\ntype B\nfn first(self: &Self) -> i64\nfn second(self: &Self) -> i64 }\n",
        "struct Host {}\nimpl Mark for Host {}\n",
    ));
    let codes = snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect::<Vec<_>>();
    assert_eq!(codes, ["TRAIT_IMPL_ASSOC_TYPE_MISMATCH"; 2]);
}
