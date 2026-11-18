use super::{diagnostic_codes, session};
use eglur_driver::{SupportProfile, TrustedCatalog};
use eglur_sema::DeclarationKind;

fn assert_codes(source: &str, expected: &[&str]) {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(source);
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert!(
        analysis.trace().missing().next().is_none(),
        "{source}\n{:?}",
        analysis.trace().missing().collect::<Vec<_>>()
    );
    let resolution = analysis.resolution().unwrap();
    let eglur_sema::PhaseProduct::Performed(facts) = resolution.ownership_facts() else {
        panic!("ownership checking was not performed");
    };
    for declaration in resolution
        .declarations()
        .modules()
        .flat_map(eglur_sema::ModuleDeclarations::declarations)
        .filter(|declaration| declaration.kind() == DeclarationKind::Function)
    {
        assert!(
            matches!(facts.body(declaration.id()), eglur_sema::FactState::Known(summary) if summary.status() == eglur_sema::OwnershipBodyStatus::Checked)
        );
    }
    let snapshot = session.into_diagnostics(None);
    assert_eq!(
        diagnostic_codes(&snapshot),
        expected,
        "{source}\n{snapshot:?}"
    );
}

fn source(width: usize, consumed: usize, selected: usize, receiver: &str, setup: &str) -> String {
    let parameters = (0..width)
        .map(|index| format!("v{index}: i64"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "fn peek(_view: &i64) -> i64 = 0\n\
         fn inspect({parameters}) -> i64 {{\n\
         {setup}\nlet root = own v{consumed}\n\
         let n = peek({receiver}.{selected})\n\
         drop_own move root\nreturn n\n}}\n"
    )
}

#[test]
fn named_tuple_projection_checks_only_the_selected_root() {
    for width in [2, 8] {
        let references = (0..width)
            .map(|index| format!("&v{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let setup = format!("let pair = ({references},)");
        for consumed in [0, width - 1] {
            for selected in 0..width {
                for receiver in ["pair", "(pair)"] {
                    let expected: &[&str] = if selected == consumed {
                        &["OWNERSHIP_USE_AFTER_DISCHARGE"]
                    } else {
                        &[]
                    };
                    assert_codes(
                        &source(width, consumed, selected, receiver, &setup),
                        expected,
                    );
                }
            }
        }
    }
}

#[test]
fn computed_receivers_still_check_unselected_written_borrows() {
    for width in [2, 8] {
        let references = (0..width)
            .map(|index| format!("&v{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let receiver = format!("({references},)");
        for consumed in [0, width - 1] {
            for selected in [0, width - 1] {
                assert_codes(
                    &source(width, consumed, selected, &receiver, ""),
                    &["OWNERSHIP_USE_AFTER_DISCHARGE"],
                );
            }
        }
    }
}

#[test]
fn reassigned_aliases_are_read_at_their_projection() {
    assert_codes(
        concat!(
            "fn peek(_view: &i64) -> i64 = 0\n",
            "fn inspect(value: i64, other: i64) -> i64 {\n",
            "  let mut pair = (&value, &other)\n",
            "  pair = (&other, &other)\n",
            "  let root = own value\n",
            "  let n = peek(pair.0)\n",
            "  drop_own move root\n",
            "  return n\n",
            "}\n",
        ),
        &[],
    );
}

#[test]
fn nested_unselected_components_do_not_pollute_selected_roots() {
    assert_codes(
        concat!(
            "fn peek(_view: &i64) -> i64 = 0\n",
            "fn inspect(first: i64, second: i64, third: i64) -> i64 {\n",
            "  let inner = (&first, &second)\n",
            "  let outer = (inner, &third)\n",
            "  let root = own first\n",
            "  let n = peek(outer.1)\n",
            "  drop_own move root\n",
            "  return n\n",
            "}\n",
        ),
        &[],
    );
}
