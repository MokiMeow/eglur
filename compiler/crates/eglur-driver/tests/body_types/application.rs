use super::*;

#[test]
fn deferred_named_argument_uses_later_evidence_without_reordering_evaluation() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (argument_type, value_type, first, second) in [
        ("Opt[T]", "i8", "None", "Some(value)"),
        ("Res[T, Str]", "Res[i8, Str]", "Err(\"bad\")", "value"),
    ] {
        let source = format!(
            "fn consume[T](left: {argument_type}, right: {argument_type}) -> Unit = ()\n\
             fn inspect(value: {value_type}) -> Unit = consume(right={first}, left={second})\n"
        );
        let mut session = session(&source);

        let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
        let resolution = analysis.resolution().unwrap();
        let (_, invocation) = resolution
            .body_types()
            .invocations()
            .find(|(_, invocation)| invocation.arguments().len() == 2)
            .expect("consume invocation is fully inferred");

        assert_eq!(
            invocation.evaluation_order(),
            [invocation.arguments()[1], invocation.arguments()[0]]
        );
        let snapshot = session.into_diagnostics(None);
        assert!(
            matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
            "{source}\n{:?}",
            diagnostic_codes(&snapshot)
        );
    }
}

#[test]
fn deferred_named_divergence_prunes_ancestor_exits_but_keeps_local_exits() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn stop() -> Never = stop()\n",
        "fn fabricate[T]() -> T = stop()\n",
        "fn finish(first: i8, local: Res[i8, NumericError], last: Never) -> Never = last\n",
        "fn consume[T](left: T, right: T) -> T = left\n",
        "fn inspect(lhs: i8, rhs: i8) -> Res[i8, NumericError] = checked {\n",
        "  consume(right=fabricate(), left=finish(lhs + rhs, checked { yield lhs + rhs }, stop()))\n",
        "  yield 0\n",
        "}\n",
    ));

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let faults = resolution
        .body_types()
        .transfers()
        .iter()
        .filter(|transfer| matches!(transfer.kind(), ControlTransferKind::CheckedFault(_)))
        .collect::<Vec<_>>();

    assert_eq!(
        resolution.body_types().checked_numeric_operations().count(),
        2
    );
    assert_eq!(
        faults.len(),
        1,
        "only the nested checked region keeps its fault"
    );
    let inspect = resolution
        .declarations()
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| {
            declaration
                .name()
                .is_some_and(|name| name.spelling() == "inspect")
        })
        .unwrap();
    assert!(!resolution.body_types().transfers().iter().any(|transfer| {
        transfer.kind() == ControlTransferKind::NormalCompletion
            && transfer.target() == &ControlRegionKey::Callable(inspect.id())
    }));
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn explicit_arguments_report_every_slot_mismatch_in_source_order() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn consume[T](left: T, right: T) -> Unit = ()\n",
        "fn invalid() -> Unit = consume::[i8](right=\"bad\", left=true)\n",
    ));

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);

    assert_eq!(
        diagnostic_codes(&snapshot),
        ["CALL_ARG_TYPE_MISMATCH", "CALL_ARG_TYPE_MISMATCH"]
    );
    for (diagnostic, actual) in snapshot.diagnostics().iter().zip(["Str", "Bool"]) {
        assert_eq!(
            diagnostic.data().get("expected_type"),
            Some(&DiagnosticDataValue::from("i8"))
        );
        assert_eq!(
            diagnostic.data().get("actual_type"),
            Some(&DiagnosticDataValue::from(actual))
        );
    }
}

#[test]
fn inference_visits_unsupported_arguments_even_when_another_argument_is_poisoned() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for arguments in [
        "need_i64(true), if true { yield 1 } else { yield 2 }",
        "if true { yield 1 } else { yield 2 }, need_i64(true)",
    ] {
        let source = format!(
            "fn need_i64(value: i64) -> i64 = value\n\
             fn consume[T](left: T, right: T) -> Unit = ()\n\
             fn invalid() -> Unit = consume({arguments})\n"
        );
        let mut session = session(&source);

        let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
        assert!(analysis.trace().missing().any(|entry| {
            entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckBranching)
        }));
        assert_eq!(
            diagnostic_codes(&session.into_diagnostics(None)),
            ["CALL_ARG_TYPE_MISMATCH"]
        );
    }
}

#[test]
fn explicit_projection_arguments_normalize_before_slot_checking() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "trait HasItem { type Item }\n",
        "struct Small {}\n",
        "impl HasItem for Small { type Item = i8 }\n",
        "fn consume[T: HasItem](value: T::Item) -> Unit = ()\n",
        "fn valid() -> Unit = consume::[Small](1)\n",
        "fn invalid() -> Unit = consume::[Small](true)\n",
    ));

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);

    assert_eq!(diagnostic_codes(&snapshot), ["CALL_ARG_TYPE_MISMATCH"]);
    assert_eq!(
        snapshot.diagnostics()[0].data().get("expected_type"),
        Some(&DiagnosticDataValue::from("i8"))
    );
}
