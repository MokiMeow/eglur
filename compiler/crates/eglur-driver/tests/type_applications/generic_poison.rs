use std::collections::BTreeMap;

use eglur_sema::{DeclarationKind, FactState};

use super::*;

#[test]
fn generic_and_header_failures_keep_roots_and_later_declarations() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for count in [8, 32, 128] {
        let names = (0..count)
            .map(|index| format!("Missing{index}"))
            .collect::<Vec<_>>();
        let missing = names.join("+");
        let tuple = names.join(",");
        let marks = (0..count)
            .map(|index| format!("Mark{index}"))
            .collect::<Vec<_>>();
        let traits = marks
            .iter()
            .map(|name| format!("trait {name} {{}}\n"))
            .collect::<String>();
        for form in [
            "unknown",
            "duplicate",
            "mixed",
            "wide",
            "known",
            "impl-both",
            "impl-negative",
            "impl-known",
        ] {
            let declaration = match form {
                "unknown" => format!("fn drive[T:{missing}]() -> Unit = ()"),
                "duplicate" => format!(
                    "trait Mark {{}}\nfn drive[T:{}]() -> Unit = ()",
                    vec!["Mark"; count].join("+")
                ),
                "mixed" => format!(
                    "{traits}fn drive[T:{}]() -> Unit = ()",
                    (0..count)
                        .map(|index| if index % 2 == 0 {
                            marks[index].clone()
                        } else {
                            names[index].clone()
                        })
                        .collect::<Vec<_>>()
                        .join("+")
                ),
                "wide" => format!(
                    "type Root = ({tuple})\ntrait Uses[A] {{}}\nfn drive[T:{}]() -> Unit = ()",
                    vec!["Uses[Root]"; count].join("+")
                ),
                "known" => format!("{traits}fn drive[T:{}]() -> Unit = ()", marks.join("+")),
                "impl-both" | "impl-negative" => format!(
                    "trait Uses[A] {{}}\nimpl{} Uses[({tuple})] for ({}) {{}}",
                    if form == "impl-negative" { " !" } else { "" },
                    (0..count)
                        .map(|index| format!("Other{index}"))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                "impl-known" => "trait Mark {}\nstruct Host {}\nimpl Mark for Host {}".to_owned(),
                _ => unreachable!(),
            };
            let source = format!("{declaration}\nfn after() -> i64 = 1\n");
            let mut session = session(&source);
            let analysis = session
                .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
                .unwrap();
            assert!(!analysis.trace().has_missing(), "{form}");
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
            let mut codes = BTreeMap::new();
            let mut actual_names = Vec::new();
            if let DiagnosticSnapshot::Rejected(snapshot) = snapshot {
                for diagnostic in snapshot.diagnostics() {
                    let code = diagnostic.code().as_str();
                    *codes.entry(code.to_owned()).or_insert(0) += 1;
                    if code == "NAME_NOT_FOUND" {
                        actual_names.push(diagnostic.data().get("name").unwrap().clone());
                    }
                    if code == "TRAIT_BOUND_INVALID" {
                        assert_eq!(
                            diagnostic.data().get("parameter").unwrap(),
                            &DiagnosticDataValue::from("T")
                        );
                        assert_eq!(
                            diagnostic.data().get("reason").unwrap(),
                            &DiagnosticDataValue::from("duplicate")
                        );
                    }
                }
            }
            let expected = match form {
                "unknown" | "wide" => vec![("NAME_NOT_FOUND", count)],
                "duplicate" => vec![("TRAIT_BOUND_INVALID", count - 1)],
                "mixed" => vec![("NAME_NOT_FOUND", count / 2)],
                "impl-both" => vec![("NAME_NOT_FOUND", 2 * count)],
                "impl-negative" => vec![
                    ("NAME_NOT_FOUND", 2 * count),
                    ("NEGATIVE_IMPL_UNSUPPORTED", 1),
                ],
                "known" | "impl-known" => Vec::new(),
                _ => unreachable!(),
            };
            assert_eq!(
                codes,
                expected
                    .into_iter()
                    .map(|(code, count)| (code.to_owned(), count))
                    .collect(),
                "{form}"
            );
            let expected_names = match form {
                "unknown" | "wide" => names.clone(),
                "mixed" => names
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| index % 2 == 1)
                    .map(|(_, name)| name.clone())
                    .collect(),
                "impl-both" | "impl-negative" => names
                    .iter()
                    .cloned()
                    .chain((0..count).map(|index| format!("Other{index}")))
                    .collect(),
                _ => Vec::new(),
            };
            assert_eq!(
                actual_names,
                expected_names
                    .into_iter()
                    .map(DiagnosticDataValue::from)
                    .collect::<Vec<_>>(),
                "{form}"
            );
        }
    }
}
