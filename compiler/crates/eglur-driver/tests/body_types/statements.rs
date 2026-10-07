use super::{body_profile, diagnostic_codes, session};
use eglur_driver::TrustedCatalog;
use eglur_sema::{CompileCapability, ControlTransferKind, FactState, TypecheckCapability};

#[test]
fn statement_preconditions_do_not_request_rejected_child_expressions() {
    let cases = [
        (
            "fn f() -> Unit { let value = 0\nvalue = [1] }\n",
            "IMMUTABLE_BINDING_ASSIGN",
        ),
        (
            "fn f() -> Unit { 0 = [1] }\n",
            "ASSIGN_TARGET_NOT_ASSIGNABLE",
        ),
        ("fn f() -> Unit { return [1] }\n", "RETURN_VALUE_FORBIDDEN"),
        ("fn f() -> Unit { var value = [1] }\n", "VAR_DECL_FORBIDDEN"),
        (
            "fn f() -> Unit { let (value, value) = [1] }\n",
            "LET_PATTERN_DUPLICATE_BINDING",
        ),
    ];
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (source, expected) in cases {
        let mut session = session(source);
        let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
        assert!(!analysis.trace().missing().any(|entry| {
            entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckContainers)
        }));
        assert_eq!(
            diagnostic_codes(&session.into_diagnostics(None)),
            [expected],
            "{source}"
        );
    }
}

#[test]
fn statement_requests_preserve_expected_types_through_nested_blocks() {
    let source = concat!(
        "fn f() -> i8 {\n",
        "  let mut value: i8 = { yield 1 }\n",
        "  value = { yield 2 }\n",
        "  return { yield value }\n",
        "}\n",
    );
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(source);
    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let transfers = resolution.body_types().transfers();
    assert_eq!(
        transfers
            .iter()
            .filter(|t| t.kind() == ControlTransferKind::Yield)
            .count(),
        3
    );
    assert_eq!(
        transfers
            .iter()
            .filter(|t| t.kind() == ControlTransferKind::Return)
            .count(),
        1
    );
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn failed_initializer_poison_is_bound_before_later_statements() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session("fn f() -> i64 { let value: i64 = true\nreturn value }\n");
    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let binding = resolution
        .names()
        .locals()
        .iter()
        .find(|binding| binding.name().spelling() == "value")
        .unwrap();
    assert!(matches!(
        resolution.body_types().local(binding.id()),
        FactState::Poisoned(_)
    ));
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["ASSIGNMENT_TYPE_MISMATCH"]
    );
}

#[test]
fn unreachable_statements_are_checked_without_enclosing_question_transfers() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut checked_session = session(concat!(
        "fn stop() -> Never = stop()\n",
        "fn source() -> Res[i64, Str] = Ok(1)\n",
        "fn f() -> Res[i64, Str] = result {\n",
        "  let mut value = 0\n",
        "  stop()\n",
        "  value = source()?\n",
        "  yield Ok(value)\n",
        "}\n",
    ));
    let analysis = checked_session
        .resolve(&body_profile(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().unwrap();
    assert!(
        !resolution
            .body_types()
            .transfers()
            .iter()
            .any(|transfer| transfer.kind() == ControlTransferKind::QuestionShortCircuit)
    );
    assert_eq!(
        diagnostic_codes(&checked_session.into_diagnostics(None)),
        Vec::<&str>::new()
    );

    let mut invalid_session =
        session("fn f() -> i64 { let mut value = 0\nreturn 0\nvalue = true }\n");
    invalid_session
        .resolve(&body_profile(), Some(&catalog))
        .unwrap();
    assert_eq!(
        diagnostic_codes(&invalid_session.into_diagnostics(None)),
        ["ASSIGNMENT_TYPE_MISMATCH"]
    );
}

#[test]
fn wide_block_failures_preserve_later_statements_bodies_and_partial_diagnostics() {
    for count in [8_usize, 32, 128] {
        for mode in ["names", "projection", "mixed-partial", "return-leading"] {
            let statements = (0..count)
                .map(|index| match mode {
                    "projection" => format!("let v{index} = x.{index}\n"),
                    "mixed-partial" if index % 2 == 0 => format!("let v{index} = [1]\n"),
                    _ => format!("let v{index} = missing{index}\n"),
                })
                .collect::<String>();
            let prefix = if mode == "return-leading" {
                "return\n"
            } else {
                ""
            };
            let source = format!(
                "fn f(x: i64) -> Unit {{ {prefix}{statements}yield () }}\nfn after() -> Unit = ()\n"
            );
            let mut session = session(&source);
            let catalog = TrustedCatalog::first_slice().unwrap();
            let analysis = session
                .resolve(
                    &eglur_driver::SupportProfile::semantic_core_v1(),
                    Some(&catalog),
                )
                .unwrap();
            assert_eq!(analysis.trace().has_missing(), mode == "mixed-partial");
            let resolution = analysis.resolution().unwrap();
            let declaration = |name: &str| {
                resolution
                    .declarations()
                    .modules()
                    .flat_map(|module| module.declarations())
                    .find(|declaration| declaration.name().is_some_and(|key| key.text() == name))
                    .unwrap()
                    .id()
            };
            assert!(matches!(
                resolution.body_types().body(declaration("f")),
                FactState::Poisoned(_)
            ));
            assert!(matches!(
                resolution.body_types().body(declaration("after")),
                FactState::Known(_)
            ));

            let expected_code = if mode == "projection" {
                "TUPLE_PROJECTION_NON_TUPLE"
            } else {
                "NAME_NOT_FOUND"
            };
            let expected_count = if mode == "mixed-partial" {
                count / 2
            } else {
                count
            };
            assert_eq!(
                diagnostic_codes(&session.into_diagnostics(None)),
                vec![expected_code; expected_count]
            );
        }
    }
}
