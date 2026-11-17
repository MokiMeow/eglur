use super::{diagnostic_codes, session};
use eglur_driver::{SupportProfile, TrustedCatalog};
use eglur_sema::{CompileCapability, FactState, TypecheckCapability};

fn check_failure_traversal(source: &str) {
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

fn check_failure_diagnostic(source: &str, code: &str) {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(source);
    session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert_eq!(diagnostic_codes(&session.into_diagnostics(None)), [code]);
}

#[test]
fn nested_result_blocks_seal_on_the_default_stack() {
    let depth = 100;
    let source = format!(
        "fn f(x: Res[i64, Str]) -> Res[i64, Str] = {}x{}\n",
        "result { yield ".repeat(depth),
        " }".repeat(depth),
    );
    check_failure_traversal(&source);
}

#[test]
fn nested_optional_result_blocks_seal_on_the_default_stack() {
    let depth = 100;
    let source = format!(
        "fn f(x: Opt[i64]) -> Opt[i64] = {}x{}\n",
        "result { yield ".repeat(depth),
        " }".repeat(depth),
    );
    check_failure_traversal(&source);
}

#[test]
fn nested_result_bindings_restore_question_targets() {
    let depth = 100;
    let source = format!(
        "fn f(x: Res[i64, Str]) -> Res[i64, Str] = {}x{}\n",
        "result { let nested: Res[i64, Str] = ".repeat(depth),
        "\nyield Ok(nested?) }".repeat(depth),
    );
    check_failure_traversal(&source);
}

#[test]
fn nested_question_operands_share_the_work_stack() {
    let depth = 100;
    let source = format!(
        "fn f(x: i64) -> Opt[i64] = result {{ yield Some({}x{}) }}\n",
        "Some(".repeat(depth),
        ")?".repeat(depth),
    );
    check_failure_traversal(&source);
}

#[test]
fn nested_checked_call_arguments_share_the_work_stack() {
    let depth = 100;
    let source = format!(
        "fn take(value: Res[i8, NumericError]) -> i8 = match value {{ Ok(x) => x, Err(e) => 0 }}\nfn f(x: i8) -> Res[i8, NumericError] = checked {{ yield {}x{} }}\n",
        "take(checked { yield ".repeat(depth),
        " })".repeat(depth),
    );
    check_failure_traversal(&source);
}

#[test]
fn nested_checked_negations_retain_fault_targets() {
    let depth = 60;
    let source = format!(
        "fn take(value: Res[i8, NumericError]) -> i8 = match value {{ Ok(x) => x, Err(e) => 0 }}\nfn f(x: i8) -> Res[i8, NumericError] = checked {{ yield {}x{} }}\n",
        "-take(checked { yield ".repeat(depth),
        " })".repeat(depth),
    );
    check_failure_traversal(&source);
}

#[test]
fn result_blocks_and_if_regions_share_the_work_stack() {
    let depth = 60;
    let source = format!(
        "fn f(flag: Bool, x: Res[i64, Str]) -> Res[i64, Str] = {}x{}\n",
        "if flag { yield result { yield ".repeat(depth),
        " } } else { yield x }".repeat(depth),
    );
    check_failure_traversal(&source);
}

#[test]
fn nested_result_inside_checked_reopens_question_authorization() {
    let depth = 60;
    let source = format!(
        "fn take(value: Res[i8, NumericError]) -> i8 = match value {{ Ok(x) => x, Err(e) => 0 }}\nfn f(x: Res[i8, NumericError]) -> Res[i8, NumericError] = result {{ yield Ok({}x?{}) }}\n",
        "take(checked { let r: Res[i8, NumericError] = result { yield Ok(".repeat(depth),
        ") }\nyield take(r) })".repeat(depth),
    );
    check_failure_traversal(&source);
}

#[test]
fn deep_result_missing_yield_keeps_its_single_diagnostic() {
    let depth = 100;
    let source = format!(
        "fn f() -> Res[i64, Str] = {}result {{}}{}\n",
        "result { let nested: Res[i64, Str] = ".repeat(depth),
        "\nyield nested }".repeat(depth),
    );
    check_failure_diagnostic(&source, "RESULT_MISSING_YIELD");
}

#[test]
fn deep_checked_context_does_not_authorize_outer_result_questions() {
    let depth = 100;
    let source = format!(
        "fn take(value: Res[i8, NumericError]) -> i8 = match value {{ Ok(x) => x, Err(e) => 0 }}\nfn f(x: Res[i8, NumericError]) -> Res[i8, NumericError] = result {{ yield Ok({}x?{}) }}\n",
        "take(checked { yield ".repeat(depth),
        " })".repeat(depth),
    );
    check_failure_diagnostic(&source, "Q_OUTSIDE_RESULT");
}

#[test]
fn deep_unsupported_result_body_stays_unperformed() {
    let depth = 100;
    let source = format!(
        "fn f() -> Res[i64, Str] = {}[1]{}\n",
        "result { yield ".repeat(depth),
        " }".repeat(depth),
    );
    let catalog = TrustedCatalog::first_slice().unwrap();
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
