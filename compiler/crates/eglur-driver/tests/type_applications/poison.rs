use super::*;
use eglur_sema::{DeclarationKind, FactState};

#[test]
fn poisoned_type_children_retain_each_diagnostic_and_later_declarations() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for count in [8, 32, 128] {
        let names = (0..count)
            .map(|index| format!("Missing{index}"))
            .collect::<Vec<_>>();
        let arguments = names.join(",");
        let parameters = (0..count)
            .map(|index| format!("T{index}"))
            .collect::<Vec<_>>()
            .join(",");
        for form in [
            "tuple",
            "function",
            "intrinsic",
            "union",
            "nominal",
            "alias",
            "trait",
            "list",
        ] {
            let declaration = match form {
                "tuple" => format!("type Bad = ({arguments})"),
                "function" => format!("type Bad = Fn({arguments}) -> Unit"),
                "intrinsic" => format!("type Bad = Res[Unit, ({arguments})]"),
                "union" => format!("type Bad = Res[Unit, Union[{arguments}]]"),
                "nominal" => format!("type Bad = Holder[{arguments}]"),
                "alias" => format!("type Bad = Erase[{arguments}]"),
                "trait" => format!("fn drive[T: Uses[{arguments}]]() -> Unit = ()"),
                "list" => format!("enum Bad {{ Item({arguments}) }}"),
                _ => unreachable!(),
            };
            let source = format!(
                "struct Holder[{parameters}] {{}}\ntype Erase[{parameters}] = Unit\ntrait Uses[{parameters}] {{}}\n{declaration}\nfn after() -> i64 = 1\n"
            );
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
            assert!(
                matches!(
                    resolution.signatures().source_declaration(after.id()),
                    FactState::Known(_)
                ),
                "{form}"
            );

            let DiagnosticSnapshot::Rejected(snapshot) = session.into_diagnostics(None) else {
                panic!("missing child types are rejected: {form}");
            };
            assert_eq!(snapshot.diagnostics().len(), count, "{form}");
            let actual = snapshot
                .diagnostics()
                .iter()
                .map(|diagnostic| {
                    assert_eq!(diagnostic.code().as_str(), "NAME_NOT_FOUND", "{form}");
                    diagnostic.data().get("name").unwrap().clone()
                })
                .collect::<Vec<_>>();
            let expected = names
                .iter()
                .map(|name| DiagnosticDataValue::from(name.as_str()))
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{form}");
        }
    }
}
