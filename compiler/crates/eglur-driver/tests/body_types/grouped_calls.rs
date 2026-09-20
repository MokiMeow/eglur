use super::*;

#[test]
fn grouped_callees_record_every_retained_expression_type() {
    let mut session = session(concat!(
        "fn id(value: i64) -> i64 = value\n",
        "fn inspect() -> i64 = ((id))(7)\n",
    ));
    let expressions = session.sources()[0]
        .hir()
        .unwrap()
        .bodies()
        .flat_map(|(_, body)| body.expressions().map(|(id, _)| id))
        .collect::<Vec<_>>();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert_eq!(analysis.trace().missing().count(), 0);
    let resolution = analysis.resolution().unwrap();
    let module = resolution.declarations().modules().next().unwrap().id();
    for expression in expressions {
        assert!(
            matches!(
                resolution
                    .body_types()
                    .expression(eglur_sema::ExpressionKey::new(module, expression)),
                FactState::Known(_)
            ),
            "missing type for {expression:?}"
        );
    }
    assert!(resolution.fingerprints().is_some());
    assert!(session.seal(None).unwrap().checked_program().is_some());
}

#[test]
fn grouped_callees_preserve_call_diagnostics_and_root_poison() {
    for (declarations, call, expected, blamed) in [
        (
            "fn f(value: i64) -> i64 = value\n",
            "(f)(true)",
            "CALL_ARG_TYPE_MISMATCH",
            "true",
        ),
        (
            "fn f(value: i64) -> i64 = value\n",
            "((f))()",
            "CALL_MISSING_ARG",
            "((f))()",
        ),
        ("", "((missing))(7)", "NAME_NOT_FOUND", "missing"),
        (
            "struct Cell {}\n",
            "(Cell {}.absent)()",
            "METHOD_NOT_FOUND",
            "absent",
        ),
        (
            "fn f!(value: i64) -> i64 { emit!(\"call\")\nyield value }\n",
            "(f!)(7)",
            "EFFECT_VIOLATION",
            "(f!)(7)",
        ),
        (
            "fn f!(value: i64) -> i64 { emit!(\"call\")\nyield value }\n",
            "7.then(((f!)))",
            "EFFECT_VIOLATION",
            "7.then(((f!)))",
        ),
    ] {
        let source =
            format!("import std::test::emit!\n{declarations}fn inspect() -> i64 = {call}\n");
        let mut session = session(&source);
        let catalog = TrustedCatalog::first_slice().unwrap();
        let analysis = session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert_eq!(analysis.trace().missing().count(), 0, "{source}");
        let snapshot = session.into_diagnostics(None);
        assert_eq!(
            diagnostic_codes(&snapshot),
            [expected],
            "{source}\n{snapshot:?}"
        );
        let span = snapshot.diagnostics()[0]
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range();
        assert_eq!(
            source[span.start().as_usize()..span.end().as_usize()].trim(),
            blamed
        );
        if expected == "CALL_ARG_TYPE_MISMATCH" {
            let diagnostic = &snapshot.diagnostics()[0];
            assert_eq!(diagnostic.data().get("expected_type"), Some(&"i64".into()));
            assert_eq!(diagnostic.data().get("actual_type"), Some(&"Bool".into()));
        }
    }
}

#[test]
fn grouped_method_and_overload_callees_keep_their_missing_capabilities() {
    for (declarations, call, capability) in [
        (
            "struct Cell { fn read(self: &Self) -> i64 = 7 }\n",
            "(Cell {}.read)()",
            TypecheckCapability::ResolveMethods,
        ),
        (
            "overload fn f(value: i64) -> i64 = value\noverload fn f(value: Str) -> Str = value\n",
            "((f))(7)",
            TypecheckCapability::ResolveOverloads,
        ),
    ] {
        let mut session = session(&format!("{declarations}fn inspect() -> i64 = {call}\n"));
        let catalog = TrustedCatalog::first_slice().unwrap();
        let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
        assert!(
            analysis
                .trace()
                .missing()
                .any(|entry| entry.capability() == CompileCapability::Typecheck(capability))
        );
        assert!(!analysis.trace().missing().any(|entry| entry.capability()
            == CompileCapability::Typecheck(TypecheckCapability::CheckClosures)));
        let sealed = session.seal(None).unwrap();
        assert!(sealed.checked_program().is_none());
        assert!(diagnostic_codes(sealed.diagnostics()).is_empty());
    }
}

#[test]
fn grouped_local_callees_remain_unsupported_not_successful() {
    for callee in ["value", "(value)", "((value))"] {
        let mut session = session(&format!("fn inspect(value: i64) -> i64 = {callee}(7)\n"));
        let catalog = TrustedCatalog::first_slice().unwrap();
        let analysis = session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert!(
            analysis.trace().missing().any(|entry| entry.capability()
                == CompileCapability::Typecheck(TypecheckCapability::CheckClosures)),
            "{callee}"
        );
        assert_eq!(
            analysis
                .resolution()
                .unwrap()
                .body_types()
                .invocations()
                .len(),
            0
        );
        let sealed = session.seal(None).unwrap();
        assert!(sealed.checked_program().is_none());
        assert!(diagnostic_codes(sealed.diagnostics()).is_empty());
    }
}

#[test]
fn grouped_consumers_preserve_ownership_discharge_checks() {
    for (calls, expected) in [
        ("(consume)(move value)", Vec::<&str>::new()),
        (
            "(consume)(move value)\n(consume)(move value)",
            vec!["OWNERSHIP_DOUBLE_MOVE"],
        ),
    ] {
        let source = format!(
            "fn consume(value: Own[i64]) -> Unit {{ drop_own move value\nyield () }}\n\
             fn inspect(value: Own[i64]) -> Unit {{ {calls}\nyield () }}\n"
        );
        let mut session = session(&source);
        let catalog = TrustedCatalog::first_slice().unwrap();
        let analysis = session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert_eq!(analysis.trace().missing().count(), 0);
        assert_eq!(
            diagnostic_codes(&session.into_diagnostics(None)),
            expected,
            "{source}"
        );
    }
}
