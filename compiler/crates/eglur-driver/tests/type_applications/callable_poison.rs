use std::collections::BTreeMap;

use eglur_sema::{DeclarationKind, FactState};

use super::*;

#[test]
fn callable_failures_keep_all_roots_without_blocking_later_declarations() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for count in [8, 32, 128] {
        for form in [
            "unknown",
            "pattern",
            "pattern-unknown",
            "mutable",
            "mixed",
            "method-unknown",
            "method-poisoned-receiver",
            "method-mutable",
            "method-effect",
            "method-receiver",
            "missing-return",
            "bang-mutable",
            "bang-method",
        ] {
            let parameters = (0..count)
                .map(|index| match form {
                    "pattern" => "_: i64".to_owned(),
                    "pattern-unknown" => format!("_: Missing{index}"),
                    "unknown" | "method-unknown" | "method-poisoned-receiver" => {
                        format!("p{index}: Missing{index}")
                    }
                    "mixed" if index % 2 == 1 => format!("p{index}: Missing{index}"),
                    _ => format!("p{index}: &mut i64"),
                })
                .collect::<Vec<_>>()
                .join(", ");
            let declaration = match form {
                "method-unknown" | "method-effect" => format!(
                    "struct Owner {{ fn drive(self: &mut Self, {parameters}) -> Unit = () }}"
                ),
                "method-mutable" => {
                    format!("struct Owner {{ fn drive(self: Self, {parameters}) -> Unit = () }}")
                }
                "method-receiver" | "method-poisoned-receiver" => {
                    format!("struct Owner {{ fn drive(head: i64, {parameters}) -> Unit = () }}")
                }
                "bang-method" => format!(
                    "struct Owner {{ fn drive!(self: &mut Self, {parameters}) -> Unit = () }}"
                ),
                "bang-mutable" => format!("fn drive!({parameters}) -> Unit = ()"),
                "missing-return" => format!("fn drive({parameters}) = ()"),
                _ => format!("fn drive({parameters}) -> Unit = ()"),
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
            let mut missing_names = Vec::new();
            if let DiagnosticSnapshot::Rejected(snapshot) = snapshot {
                for diagnostic in snapshot.diagnostics() {
                    let code = diagnostic.code().as_str();
                    *codes.entry(code.to_owned()).or_insert(0) += 1;
                    if code == "NAME_NOT_FOUND" {
                        missing_names.push(diagnostic.data().get("name").unwrap().clone());
                    }
                }
            }
            let expected_codes = match form {
                "unknown" | "method-unknown" | "method-poisoned-receiver" => {
                    vec![("NAME_NOT_FOUND", count)]
                }
                "pattern" => vec![("PARAM_PATTERN_UNSUPPORTED", count)],
                "pattern-unknown" => vec![
                    ("NAME_NOT_FOUND", count),
                    ("PARAM_PATTERN_UNSUPPORTED", count),
                ],
                "mixed" => vec![("NAME_NOT_FOUND", count / 2)],
                "mutable" | "method-mutable" => vec![("NON_BANG_MUT_PARAM_FORBIDDEN", count)],
                "method-effect" => vec![("EFFECT_SIGNATURE", 1)],
                "method-receiver" => vec![("METHOD_RECEIVER_INVALID", 1)],
                "missing-return" => vec![("FN_RETURN_TYPE_REQUIRED", 1)],
                "bang-mutable" | "bang-method" => Vec::new(),
                _ => unreachable!(),
            };
            assert_eq!(
                codes,
                expected_codes
                    .into_iter()
                    .map(|(code, count)| (code.to_owned(), count))
                    .collect(),
                "{form}, count={count}"
            );
            let expected_names = match form {
                "unknown" | "method-unknown" | "method-poisoned-receiver" | "pattern-unknown" => {
                    (0..count).collect()
                }
                "mixed" => (0..count).filter(|index| index % 2 == 1).collect(),
                _ => Vec::new(),
            };
            assert_eq!(
                missing_names,
                expected_names
                    .into_iter()
                    .map(|index| DiagnosticDataValue::from(format!("Missing{index}")))
                    .collect::<Vec<_>>(),
                "{form}, count={count}"
            );
        }
    }
}
