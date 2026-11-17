use super::{branching_profile, diagnostic_codes, session};
use eglur_diagnostics::DiagnosticDataValue;
use eglur_driver::TrustedCatalog;
use eglur_sema::{FactState, PrimitiveType, TypeKind};

#[test]
fn duplicate_aliases_report_the_later_binding_not_an_internal_collision() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (ty, pattern, later) in [
        ("i64", "x @ x", "x"),
        ("i64", "x @ (x)", "x)"),
        ("i64", "x @ x @ x", "x @ x"),
        ("(i64, i64)", "x @ (x, _)", "x, _"),
        ("(i64, i64)", "(x @ x, _)", "x, _"),
        ("Opt[i64]", "x @ Some(x)", "x)"),
        ("Opt[i64]", "Some(x @ x)", "x)"),
    ] {
        let source = format!("fn f(v: {ty}) -> i64 = match v {{ {pattern} => 0, _ => 0 }}\n");
        let mut rejected = session(&source);
        rejected
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        let snapshot = rejected.into_diagnostics(None);
        assert_eq!(
            diagnostic_codes(&snapshot),
            ["PATTERN_DUPLICATE_BINDING"],
            "{pattern}"
        );
        let diagnostic = &snapshot.diagnostics()[0];
        assert_eq!(
            diagnostic.data().get("name"),
            Some(&DiagnosticDataValue::from("x"))
        );
        let range = diagnostic
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range();
        let pattern_start = source.find(pattern).unwrap();
        let expected = pattern_start + pattern.rfind(later).unwrap();
        assert_eq!(range.start().as_usize(), expected, "{pattern}");
        assert_eq!(
            &source[range.start().as_usize()..range.end().as_usize()],
            "x"
        );
    }
}

#[test]
fn alias_canonicalization_preserves_pattern_diagnostic_precedence() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (source, expected) in [
        (
            "fn f(v: i64) -> i64 = match v { x @ (x, _) => 0 }",
            "PATTERN_TYPE_MISMATCH",
        ),
        (
            "fn f(v: (i64, i64)) -> i64 = match v { x @ (x, _, _) => 0 }",
            "PATTERN_ARITY_MISMATCH",
        ),
        (
            "fn f(v: Opt[i64]) -> i64 = match v { x @ Some(x, x) => 0 }",
            "PATTERN_ARITY_MISMATCH",
        ),
        (
            "fn f(v: i64) -> i64 = match v { x @ Missing(x) => 0 }",
            "NAME_NOT_FOUND",
        ),
        (
            "fn f() -> i64 { let x @ x = 1\nyield 0 }",
            "LET_PATTERN_DUPLICATE_BINDING",
        ),
        (
            "fn f() -> i64 { let x @ y = 1\nyield 0 }",
            "LET_PATTERN_UNSUPPORTED",
        ),
        ("fn f(x @ x: i64) -> i64 = 0", "PARAM_PATTERN_UNSUPPORTED"),
        (
            "fn f() -> i64 { let x = 0\nlet x @ x = 1\nyield 0 }",
            "LOCAL_BINDING_REDECLARATION",
        ),
    ] {
        let mut rejected = session(source);
        rejected
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        assert_eq!(
            diagnostic_codes(&rejected.into_diagnostics(None)),
            [expected],
            "{source}"
        );
    }
}

#[test]
fn distinct_aliases_remain_valid_bindings() {
    let source = "fn f(v: i64) -> i64 = match v { outer @ inner @ x => outer }\n";
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut accepted = session(source);
    let analysis = accepted
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().expect("body typing is selected");
    assert_eq!(resolution.names().locals().len(), 4);
    for binding in resolution.names().locals() {
        let FactState::Known(ty) = resolution.body_types().local(binding.id()) else {
            panic!("alias binding has no known type");
        };
        assert!(matches!(
            resolution.signatures().types().kind(ty.ty()).unwrap(),
            TypeKind::Primitive(PrimitiveType::I64)
        ));
    }
    assert!(diagnostic_codes(&accepted.into_diagnostics(None)).is_empty());
}
