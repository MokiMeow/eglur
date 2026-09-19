use super::{diagnostic_codes, session};
use eglur_driver::{CompilerSession, SupportProfile, TrustedCatalog};
use eglur_sema::{CompileCapability, FactState, TypecheckCapability};

fn check_branches(mut session: CompilerSession) {
    let catalog = TrustedCatalog::first_slice().unwrap();
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
fn nested_if_branches_seal_on_the_default_stack() {
    let depth = 240;
    let source = format!(
        "fn f(flag: Bool) -> i64 = {}1{}\n",
        "if flag { yield ".repeat(depth),
        " } else { yield 0 }".repeat(depth),
    );
    check_branches(session(&source));
}

#[test]
fn nested_if_conditions_seal_on_the_default_stack() {
    let depth = 100;
    let source = format!(
        "fn id[T](x: T) -> T = x\nfn f(flag: Bool) -> Bool = {}flag{}\n",
        "if id(".repeat(depth),
        ") { yield true } else { yield false }".repeat(depth),
    );
    check_branches(session(&source));
}

#[test]
fn nested_match_arms_seal_on_the_default_stack() {
    let depth = 100;
    let source = format!(
        "fn f(x: i64) -> i64 = {}x{}\n",
        "match x { value => { yield ".repeat(depth),
        " } }".repeat(depth),
    );
    check_branches(session(&source));
}

#[test]
fn nested_match_scrutinees_seal_on_the_default_stack() {
    let depth = 100;
    let source = format!(
        "fn id[T](x: T) -> T = x\nfn f(x: i64) -> i64 = {}x{}\n",
        "match id(".repeat(depth),
        ") { value => value }".repeat(depth),
    );
    check_branches(session(&source));
}

#[test]
fn nested_match_guards_seal_on_the_default_stack() {
    let depth = 60;
    let source = format!(
        "fn id[T](x: T) -> T = x\nfn f(flag: Bool) -> Bool = {}flag{}\n",
        "match flag { value if id(".repeat(depth),
        ") => value, _ => false }".repeat(depth),
    );
    check_branches(session(&source));
}

#[test]
fn nested_empty_match_scrutinees_seal_on_the_default_stack() {
    let depth = 100;
    let source = format!(
        "fn id[T](x: T) -> T = x\nfn f(x: Never) -> Never = {}x{}\n",
        "match id(".repeat(depth),
        ") {}".repeat(depth),
    );
    check_branches(session(&source));
}

#[test]
fn branches_calls_constructors_and_projections_share_the_work_stack() {
    let depth = 40;
    let source = format!(
        "struct Wrap[T] {{ field value: T }}\nfn id[T](value: T) -> T = value\nfn f(flag: Bool, x: i64) -> i64 = {}x{}\n",
        "id((if flag { yield match x { value => Wrap { value: ".repeat(depth),
        " }.value } } else { yield x },).0)".repeat(depth),
    );
    check_branches(session(&source));
}

#[test]
fn deep_branch_failure_preserves_one_diagnostic() {
    let depth = 60;
    let source = format!(
        "fn f(flag: Bool, x: i64) -> i64 = {}x.0{}\n",
        "if flag { yield match x { _ => ".repeat(depth),
        " } } else { yield x }".repeat(depth),
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
fn deep_unsupported_guard_stays_unperformed() {
    let depth = 60;
    let source = format!(
        "fn id[T](x: T) -> T = x\nfn f(flag: Bool) -> Bool = {}[true]{}\n",
        "match flag { _ if id(".repeat(depth),
        ") => true, _ => false }".repeat(depth),
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

#[test]
fn poisoned_branches_are_retained_after_unsupported_conditions_and_guards() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for source in [
        "fn f(x: i64) -> i64 = if [true] { yield x.0 } else { yield x.1 }\n",
        "fn f(x: i64) -> i64 = match x { _ if [true] => x.0, _ => x.1 }\n",
    ] {
        let mut session = session(source);
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
            FactState::Poisoned(_)
        ));
        assert_eq!(
            diagnostic_codes(&session.into_diagnostics(None)),
            ["TUPLE_PROJECTION_NON_TUPLE", "TUPLE_PROJECTION_NON_TUPLE"]
        );
    }
}
