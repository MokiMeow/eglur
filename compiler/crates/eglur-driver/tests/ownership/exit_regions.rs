use super::{diagnostic_codes, session};
use eglur_diagnostics::DiagnosticSnapshot;
use eglur_driver::{SupportProfile, TrustedCatalog};
use eglur_sema::{DeclarationKind, FactState, OwnershipBodyStatus, PhaseProduct};

fn checked(source: &str, expected: &[&str]) -> (DiagnosticSnapshot, usize) {
    let mut session = session(source);
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert!(analysis.trace().missing().next().is_none(), "{source}");
    let resolution = analysis.resolution().unwrap();
    let PhaseProduct::Performed(facts) = resolution.ownership_facts() else {
        panic!("ownership checking was not performed");
    };
    let mut roots = 0;
    let mut missing = Vec::new();
    for declaration in resolution
        .declarations()
        .modules()
        .flat_map(eglur_sema::ModuleDeclarations::declarations)
        .filter(|declaration| declaration.kind() == DeclarationKind::Function)
    {
        match facts.body(declaration.id()) {
            FactState::Known(summary) => {
                assert_eq!(summary.status(), OwnershipBodyStatus::Checked);
                roots += summary.roots().len();
            }
            state => missing.push(format!("{:?}: {state:?}", declaration.id())),
        }
    }
    let snapshot = session.into_diagnostics(None);
    assert_eq!(
        diagnostic_codes(&snapshot),
        expected,
        "{source}\n{snapshot:?}"
    );
    assert!(missing.is_empty(), "{source}\n{missing:?}\n{snapshot:?}");
    (snapshot, roots)
}

#[test]
fn sibling_region_cleanup_preserves_historical_root_facts() {
    for count in [1, 16, 64] {
        let blocks = (0..count)
            .map(|index| {
                format!(
                    "{{ let value{index}: i64 = 0\n\
                     let root{index} = own value{index}\n\
                     drop_own move root{index}\nyield () }}\n"
                )
            })
            .collect::<String>();
        let source = format!("fn inspect() -> Unit {{\n{blocks}yield ()\n}}\n");
        let (_, roots) = checked(&source, &[]);
        assert_eq!(roots, count);
    }
}

#[test]
fn inner_exits_preserve_outer_availability_and_aliases() {
    checked(
        concat!(
            "fn peek(value: &i64) -> i64 = 0\n",
            "fn inspect(value: i64) -> i64 {\n",
            "  let alias = &value\n",
            "  let root = own value\n",
            "  { yield () }\n",
            "  let observed = peek(alias)\n",
            "  drop_own move root\n",
            "  return observed\n",
            "}\n",
        ),
        &["OWNERSHIP_USE_AFTER_DISCHARGE"],
    );
}

#[test]
fn ancestor_exits_keep_binding_order_for_the_selected_failure() {
    let (snapshot, _) = checked(
        concat!(
            "fn inspect(outer: Own[i64]) -> Unit {\n",
            "  { let value: i64 = 0\n",
            "    let inner = own value\n",
            "    return }\n",
            "}\n",
        ),
        &["OWNERSHIP_PATH_UNBALANCED"],
    );
    assert_eq!(
        snapshot.diagnostics()[0].message(),
        "ownership path for `outer` is not discharged exactly once before its obligation region exits"
    );
}

#[test]
fn unexecuted_later_bindings_do_not_introduce_obligations() {
    checked(
        concat!(
            "fn inspect(flag: Bool, value: i64) -> Unit {\n",
            "  if flag { return } else { yield () }\n",
            "  let root = own value\n",
            "  drop_own move root\n",
            "  yield ()\n",
            "}\n",
        ),
        &[],
    );
}
