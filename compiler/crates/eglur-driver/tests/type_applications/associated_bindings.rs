use std::collections::BTreeMap;

use eglur_sema::{DeclarationKind, FactState};

use super::*;

#[test]
fn binding_value_cycle_and_structure_failures_preserve_later_declarations() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for count in [8, 32, 128] {
        for mode in ["values", "reference", "cycle-reference", "mixed-cycle-late"] {
            let required = (0..count)
                .map(|index| format!("type A{index}\n"))
                .collect::<String>();
            let supplied = (0..count)
                .map(|index| {
                    let ty = match mode {
                        "values" => format!("Missing{index}"),
                        "reference" => "&i64".into(),
                        "cycle-reference" => format!("&Self::A{}", (index + 1) % count),
                        "mixed-cycle-late" if index < count / 2 => {
                            format!("Self::A{}", (index + 1) % (count / 2))
                        }
                        "mixed-cycle-late" => "&i64".into(),
                        _ => unreachable!(),
                    };
                    format!("type A{index} = {ty}\n")
                })
                .collect::<String>();
            let text = format!(
                "trait Mark {{ {required} }}\nstruct Host {{}}\nimpl Mark for Host {{ {supplied} }}\nfn after() -> i64 = 1\n"
            );
            let mut session = session(&text);
            let analysis = session
                .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
                .unwrap();
            assert!(!analysis.trace().has_missing());
            let resolution = analysis.resolution().unwrap();
            let declarations = resolution.declarations().modules().next().unwrap();
            let after = declarations
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

            let mut expected = BTreeMap::new();
            match mode {
                "values" => {
                    expected.insert("NAME_NOT_FOUND", count);
                }
                "reference" => {
                    expected.insert("REFERENCE_TYPE_POSITION_FORBIDDEN", count);
                }
                "cycle-reference" => {
                    expected.insert("ASSOCIATED_TYPE_CYCLE", 1);
                }
                "mixed-cycle-late" => {
                    expected.insert("ASSOCIATED_TYPE_CYCLE", 1);
                    expected.insert("REFERENCE_TYPE_POSITION_FORBIDDEN", count / 2);
                }
                _ => unreachable!(),
            }
            let snapshot = session.into_diagnostics(None);
            assert!(matches!(snapshot, DiagnosticSnapshot::Rejected(_)));
            let mut actual = BTreeMap::new();
            for diagnostic in snapshot.diagnostics() {
                *actual.entry(diagnostic.code().as_str()).or_insert(0) += 1;
            }
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn missing_associated_types_retain_distinct_obligations_at_impl_blame() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for count in [2, 8, 32] {
        let members = (0..count)
            .map(|index| format!("type A{index}\n"))
            .collect::<String>();
        let text = format!(
            "trait Mark {{ {members} }}\nstruct Host {{}}\nimpl Mark for Host {{}}\nfn after() -> i64 = 1\n"
        );
        let mut session = session(&text);
        let analysis = session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert!(!analysis.trace().has_missing());
        let resolution = analysis.resolution().unwrap();
        let after = resolution
            .declarations()
            .modules()
            .next()
            .unwrap()
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
        let DiagnosticSnapshot::Rejected(snapshot) = snapshot else {
            panic!("missing associated bindings reject analysis");
        };
        let diagnostics = snapshot.diagnostics();
        assert_eq!(diagnostics.len(), count);
        let first_span = diagnostics[0].primary_span().as_source().unwrap();
        let start = first_span.file_span().range().start().get() as usize;
        let end = first_span.file_span().range().end().get() as usize;
        assert_eq!(&text[start..end], "impl Mark for Host {}");

        let mut actual = diagnostics
            .iter()
            .map(|diagnostic| {
                assert_eq!(diagnostic.code().as_str(), "TRAIT_IMPL_ASSOC_TYPE_MISMATCH");
                assert_eq!(diagnostic.primary_span().as_source().unwrap(), first_span);
                assert_eq!(
                    diagnostic.data().get("trait"),
                    Some(&DiagnosticDataValue::from("Mark"))
                );
                assert_eq!(
                    diagnostic.data().get("reason"),
                    Some(&DiagnosticDataValue::from("missing"))
                );
                diagnostic.data().get("associated_type").unwrap().clone()
            })
            .collect::<Vec<_>>();
        actual.sort();
        let mut expected = (0..count)
            .map(|index| DiagnosticDataValue::from(format!("A{index}")))
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(actual, expected);
    }
}

#[test]
fn missing_bindings_precede_supplied_value_errors() {
    for value in ["Missing", "&i64", "Self::Keep"] {
        let source = format!(
            "trait Mark {{ type A\ntype B\ntype Keep }}\nstruct Host {{}}\nimpl Mark for Host {{ type Keep = {value} }}\n"
        );
        let (codes, disposition) = compile(&source);
        assert_eq!(codes, ["TRAIT_IMPL_ASSOC_TYPE_MISMATCH"; 2]);
        assert_ne!(disposition, CompileDisposition::Succeeded);
    }
}

#[test]
fn associated_binding_mismatch_kinds_keep_their_independent_roots() {
    for (members, bindings, expected) in [
        (
            "type A\ntype B",
            "type Unknown = Missing",
            BTreeMap::from([("missing", 2), ("unknown", 1)]),
        ),
        (
            "type A",
            "type A = i64\ntype A = Missing",
            BTreeMap::from([("duplicate", 1)]),
        ),
        (
            "type A\ntype B",
            "type A = i64\ntype B = Bool",
            BTreeMap::new(),
        ),
    ] {
        let text = format!(
            "trait Mark {{ {members} }}\nstruct Host {{}}\nimpl Mark for Host {{ {bindings} }}\n"
        );
        let catalog = TrustedCatalog::first_slice().unwrap();
        let mut session = session(&text);
        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        let snapshot = session.into_diagnostics(None);
        let mut actual = BTreeMap::new();
        for diagnostic in snapshot.diagnostics() {
            assert_eq!(diagnostic.code().as_str(), "TRAIT_IMPL_ASSOC_TYPE_MISMATCH");
            let DiagnosticDataValue::String(reason) = diagnostic.data().get("reason").unwrap()
            else {
                panic!("mismatch reason is text");
            };
            *actual.entry(reason.as_ref()).or_insert(0) += 1;
        }
        assert_eq!(actual, expected);
    }
}
