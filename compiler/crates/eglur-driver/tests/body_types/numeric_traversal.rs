use super::{diagnostic_codes, session};
use eglur_driver::{SupportProfile, TrustedCatalog};
use eglur_sema::{CompileCapability, FactState, TypecheckCapability};

fn check_numeric_traversal(source: &str) {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(source);
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let missing = analysis.trace().missing().collect::<Vec<_>>();
    assert!(missing.is_empty(), "{missing:?}");
    assert!(analysis.fingerprints().is_some());

    let diagnostics = session.into_diagnostics(None);
    assert!(
        diagnostic_codes(&diagnostics).is_empty(),
        "{:?}",
        diagnostics.diagnostics()
    );
}

#[test]
fn grouped_if_conditions_seal_on_the_default_stack() {
    let depth = 100;
    let source = format!(
        "fn f(flag: Bool) -> Bool = {}flag{}\n",
        "if (".repeat(depth),
        ") { yield true } else { yield false }".repeat(depth),
    );
    check_numeric_traversal(&source);
}

#[test]
fn grouped_match_scrutinees_seal_on_the_default_stack() {
    let depth = 100;
    let source = format!(
        "fn f(x: i64) -> i64 = {}x{}\n",
        "match (".repeat(depth),
        ") { value => value }".repeat(depth),
    );
    check_numeric_traversal(&source);
}

#[test]
fn grouped_match_guards_seal_on_the_default_stack() {
    let depth = 60;
    let source = format!(
        "fn f(flag: Bool) -> Bool = {}flag{}\n",
        "match flag { value if (".repeat(depth),
        ") => value, _ => false }".repeat(depth),
    );
    check_numeric_traversal(&source);
}

#[test]
fn grouped_empty_match_scrutinees_seal_on_the_default_stack() {
    let depth = 100;
    let source = format!(
        "fn f(x: Never) -> Never = {}x{}\n",
        "match (".repeat(depth),
        ") {}".repeat(depth),
    );
    check_numeric_traversal(&source);
}

#[test]
fn binary_block_operands_share_the_work_stack() {
    let depth = 100;
    let source = format!(
        "fn f(x: i64) -> Res[i64, NumericError] = checked {{ yield {}x{} }}\n",
        "x + { yield ".repeat(depth),
        " }".repeat(depth),
    );
    check_numeric_traversal(&source);
}

#[test]
fn binary_call_arguments_share_the_work_stack() {
    let depth = 100;
    let source = format!(
        "fn id[T](value: T) -> T = value\nfn f(x: i64) -> Res[i64, NumericError] = checked {{ yield {}x{} }}\n",
        "id(x + ".repeat(depth),
        ")".repeat(depth),
    );
    check_numeric_traversal(&source);
}

#[test]
fn negated_block_operands_share_the_work_stack() {
    let depth = 100;
    let source = format!(
        "fn f(x: i64) -> Res[i64, NumericError] = checked {{ yield {}x{} }}\n",
        "- { yield ".repeat(depth),
        " }".repeat(depth),
    );
    check_numeric_traversal(&source);
}

#[test]
fn numeric_constructor_fields_share_the_work_stack() {
    let depth = 60;
    let source = format!(
        "struct Wrap[T] {{ field value: T }}\nfn f(x: i64) -> Res[i64, NumericError] = checked {{ yield {}x{} }}\n",
        "Wrap { value: x + ".repeat(depth),
        " }.value".repeat(depth),
    );
    check_numeric_traversal(&source);
}

#[test]
fn nested_literal_probes_retain_opposite_operand_context() {
    let depth = 100;
    let source = format!(
        "fn id[T](value: T) -> T = value\nfn f(x: i8) -> Res[i8, NumericError] = checked {{ let value = {}x{}\nyield value }}\n",
        "1 + id(".repeat(depth),
        ")".repeat(depth),
    );
    check_numeric_traversal(&source);
}

#[test]
fn deferred_opposite_operand_probe_resumes_with_literal_context() {
    check_numeric_traversal(concat!(
        "fn fabricate[T]() -> T = fabricate()\n",
        "fn f() -> Res[i64, NumericError] = checked {\n",
        "  let value = 1 + fabricate()\n",
        "  yield value\n",
        "}\n",
    ));
}

#[test]
fn deep_numeric_operand_poison_does_not_add_cascading_diagnostics() {
    let depth = 100;
    let source = format!(
        "fn f(x: i64) -> Res[i64, NumericError] = checked {{ yield {}x.0{} }}\n",
        "x + { yield ".repeat(depth),
        " }".repeat(depth),
    );
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(&source);
    session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["TUPLE_PROJECTION_NON_TUPLE"]
    );
}

#[test]
fn deep_unsupported_numeric_leaves_stay_unperformed() {
    let depth = 60;
    let sources = [
        format!(
            "fn f(x: i64) -> Res[i64, NumericError] = checked {{ yield {}[1]{} }}\n",
            "x + { yield ".repeat(depth),
            " }".repeat(depth),
        ),
        format!(
            "fn f(flag: Bool) -> Bool = {}[true]{}\n",
            "match flag { _ if (".repeat(depth),
            ") => true, _ => false }".repeat(depth),
        ),
    ];
    let catalog = TrustedCatalog::first_slice().unwrap();
    for source in sources {
        let mut session = session(&source);
        let analysis = session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert!(analysis.trace().missing().any(|entry| {
            entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckContainers)
        }));
        let resolution = analysis.resolution().unwrap();
        let function = resolution
            .declarations()
            .modules()
            .flat_map(|module| module.declarations())
            .find(|declaration| {
                declaration
                    .name()
                    .is_some_and(|name| name.spelling() == "f")
            })
            .unwrap();
        assert!(matches!(
            resolution.body_types().body(function.id()),
            FactState::Unperformed
        ));
        assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
    }
}
