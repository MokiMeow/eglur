use std::collections::BTreeMap;

use eglur_sema::{DeclarationKind, FactState};

use super::*;

#[test]
fn enum_payload_and_variant_failures_keep_roots_and_later_declarations() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for count in [8, 32, 128] {
        let names = (0..count)
            .map(|index| format!("Missing{index}"))
            .collect::<Vec<_>>();
        let tuple = names.join(",");
        for mode in [
            "named",
            "named-mixed",
            "named-known",
            "named-shared",
            "named-wide",
            "variants",
            "variants-mixed",
            "variants-known",
            "variants-shared",
            "variants-wide",
            "variants-named",
            "variants-public",
        ] {
            let fields = |types: &[String]| {
                types
                    .iter()
                    .enumerate()
                    .map(|(index, ty)| format!("f{index}:{ty}"))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            let text = match mode {
                "named" => format!("enum E {{ Item {{ {}, end:Bool }} }}", fields(&names)),
                "named-mixed" => format!(
                    "enum E {{ Item {{ {} }} }}",
                    fields(
                        &(0..count)
                            .map(|index| if index % 2 == 0 {
                                "i64".to_owned()
                            } else {
                                names[index].clone()
                            })
                            .collect::<Vec<_>>()
                    )
                ),
                "named-known" => format!(
                    "enum E {{ Item {{ {} }} }}",
                    fields(&vec!["i64".to_owned(); count])
                ),
                "named-shared" | "named-wide" => format!(
                    "type Root = {}\nenum E {{ Item {{ {} }} }}",
                    if mode == "named-shared" {
                        "Missing".to_owned()
                    } else {
                        format!("({tuple})")
                    },
                    fields(&vec!["Root".to_owned(); count])
                ),
                "variants" => format!(
                    "enum E {{ {}\nLast }}",
                    names
                        .iter()
                        .enumerate()
                        .map(|(index, ty)| format!("V{index}({ty})"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                "variants-mixed" => format!(
                    "enum E {{ {} }}",
                    (0..count)
                        .map(|index| if index % 2 == 0 {
                            format!("V{index}")
                        } else {
                            format!("V{index}({})", names[index])
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                "variants-known" => format!(
                    "enum E {{ {} }}",
                    (0..count)
                        .map(|index| format!("V{index}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                "variants-shared" | "variants-wide" => format!(
                    "type Root = {}\nenum E {{ {} }}",
                    if mode == "variants-shared" {
                        "Missing".to_owned()
                    } else {
                        format!("({tuple})")
                    },
                    (0..count)
                        .map(|index| format!("V{index}(Root)"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                "variants-named" => format!(
                    "enum E {{ {} }}",
                    names
                        .iter()
                        .enumerate()
                        .map(|(index, ty)| format!("V{index} {{ f:{ty} }}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                "variants-public" => format!(
                    "enum E {{ {} }}",
                    (0..count)
                        .map(|index| format!("pub V{index}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                _ => unreachable!(),
            };
            let source = format!("{text}\nfn after() -> i64 = 1\n");
            let mut session = session(&source);
            let analysis = session
                .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
                .unwrap();
            assert!(!analysis.trace().has_missing(), "{mode}");
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
                }
            }
            let expected = match mode {
                "named-known" | "variants-known" => BTreeMap::new(),
                "named-shared" | "variants-shared" => {
                    BTreeMap::from([("NAME_NOT_FOUND".to_owned(), 1)])
                }
                "named-mixed" | "variants-mixed" => {
                    BTreeMap::from([("NAME_NOT_FOUND".to_owned(), count / 2)])
                }
                "variants-public" => {
                    BTreeMap::from([("ENUM_VARIANT_VISIBILITY_FORBIDDEN".to_owned(), count)])
                }
                _ => BTreeMap::from([("NAME_NOT_FOUND".to_owned(), count)]),
            };
            assert_eq!(codes, expected, "{mode}");
            let expected_names = match mode {
                "named-known" | "variants-known" | "variants-public" => Vec::new(),
                "named-shared" | "variants-shared" => vec!["Missing".to_owned()],
                "named-mixed" | "variants-mixed" => names
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| index % 2 == 1)
                    .map(|(_, name)| name.clone())
                    .collect(),
                _ => names.clone(),
            };
            assert_eq!(
                actual_names,
                expected_names
                    .into_iter()
                    .map(DiagnosticDataValue::from)
                    .collect::<Vec<_>>(),
                "{mode}"
            );
        }
    }
}
