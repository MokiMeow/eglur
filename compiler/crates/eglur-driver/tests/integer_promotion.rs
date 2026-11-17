use eglur_driver::{
    CompilerSession, CompletedExecution, EvaluationLimits, ExecutionAttempt, ExecutionOutcome,
    FileSetRequest, ProgramMode, RuntimeEvent, SealedCompilation, SessionOptions, SupportProfile,
    TrustedCatalog, Value, execute,
};
use eglur_sema::{CheckedNumericOperator, ControlTransferKind, NumericFaultCause, PrimitiveType};

fn compile(source: &str) -> SealedCompilation {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    session
        .resolve_for(
            ProgramMode::Executable,
            &SupportProfile::semantic_core_v1(),
            Some(&TrustedCatalog::first_slice().unwrap()),
        )
        .unwrap();
    session.seal(None).unwrap()
}

fn run(source: &str) -> CompletedExecution {
    let compiled = compile(source);
    let program = compiled.checked_program().unwrap_or_else(|| {
        panic!(
            "{source}\n{:?}\n{:?}",
            compiled.diagnostics(),
            compiled.trace()
        )
    });
    match execute(
        program,
        &SupportProfile::semantic_core_v1(),
        &TrustedCatalog::first_slice().unwrap(),
        EvaluationLimits::default(),
    ) {
        ExecutionAttempt::Executed(ExecutionOutcome::Completed(result)) => result,
        other => panic!("{source}\n{other:?}"),
    }
}

fn assert_integer(result: &CompletedExecution, dtype: PrimitiveType, expected: i128) {
    assert_eq!(result.process_status(), 0, "{result:?}");
    let Value::Variant { payload, .. } = result.value() else {
        panic!("expected Ok")
    };
    let [Value::Integer(value)] = payload.as_ref() else {
        panic!("expected integer")
    };
    assert_eq!(value.primitive(), dtype);
    assert_eq!(value.value(), expected);
}

#[test]
fn all_fixed_width_pairs_share_constant_and_runtime_promotion() {
    use PrimitiveType::{I8, I16, I32, I64, U8, U16, U32, U64};
    let ordered = [U8, I8, U16, I16, U32, I32, U64, I64];
    for (left_index, left) in ordered.into_iter().enumerate() {
        for (right_index, right) in ordered.into_iter().enumerate() {
            let destination = ordered[left_index.max(right_index)];
            for (operator, expected) in [("+", 8), ("-", 4), ("*", 12), ("/", 3), ("%", 0)] {
                let source = format!(
                    "import std::test::emit!\ntype L = {}\ntype R = {}\nconst A: L = 6\nconst B: R = 2\nconst C: {} = A {operator} B\nfn calculate(a: L, b: R) -> Res[{}, NumericError] = checked {{ yield a {operator} b }}\nfn main!() -> Res[{}, NumericError] {{ emit!(to_str(C))\nyield calculate(A, B) }}\n",
                    left.spelling(),
                    right.spelling(),
                    destination.spelling(),
                    destination.spelling(),
                    destination.spelling(),
                );
                let result = run(&source);
                assert_integer(&result, destination, expected);
                assert_eq!(
                    result.events(),
                    [RuntimeEvent::Emitted(expected.to_string().into())]
                );
            }
        }
    }
}

#[test]
fn promotion_conversion_faults_precede_arithmetic_faults() {
    for (left, right, a, b, destination) in [
        ("u8", "i8", "255", "0", "i8"),
        ("i8", "u8", "0", "255", "i8"),
        ("i8", "u16", "-1", "0", "u16"),
        ("u16", "i8", "0", "-1", "u16"),
        ("u64", "i64", "18446744073709551615", "0", "i64"),
    ] {
        for operator in ["*", "/", "%"] {
            let source = format!(
                "fn calc(a: {left}, b: {right}) -> Res[{destination}, NumericError] = checked {{ yield a {operator} b }}\nfn main!() -> Res[{destination}, NumericError] = calc({a}, {b})\n"
            );
            let result = run(&source);
            assert_eq!(result.process_status(), 1);
            assert_eq!(result.value_diagnostics().len(), 1);
            assert_eq!(result.value_diagnostics()[0].code(), "INT_OVERFLOW");
            assert_eq!(
                result.value_diagnostics()[0].data()["dtype"].as_ref(),
                destination
            );
            for checked in [false, true] {
                let expression = format!("A {operator} B");
                let expression = if checked {
                    format!("checked {{ yield {expression} }}")
                } else {
                    expression
                };
                let source = format!(
                    "const A: {left} = {a}\nconst B: {right} = {b}\nfn main!() -> Unit {{ let value = {expression}\nyield () }}\n"
                );
                let compiled = compile(&source);
                assert_eq!(compiled.trace().missing().count(), 0, "{source}");
                let diagnostics = compiled.diagnostics().diagnostics();
                assert_eq!(diagnostics.len(), 1, "{source}\n{diagnostics:?}");
                assert_eq!(diagnostics[0].code().as_str(), "INT_OVERFLOW");
                assert_eq!(diagnostics[0].data()["dtype"], destination.into());
            }
        }
    }
}

#[test]
fn total_exemptions_require_safe_promotion_conversions() {
    for (left, right, argument, divisor, destination, expected) in [
        ("u8", "i16", "255", "2", PrimitiveType::I16, 127),
        ("i8", "i16", "-128", "2", PrimitiveType::I16, -64),
        ("u16", "i8", "10", "2", PrimitiveType::U16, 5),
    ] {
        let source = format!(
            "const D: {right} = {divisor}\nfn total(x: {left}) -> {} = x / D\nfn main!() -> Res[{}, NumericError] = Ok(total({argument}))\n",
            destination.spelling(),
            destination.spelling()
        );
        assert_integer(&run(&source), destination, expected);
    }
    for (left, right, destination) in [
        ("u8", "i8", "i8"),
        ("i8", "u16", "u16"),
        ("u64", "i64", "i64"),
    ] {
        for operator in ["/", "%"] {
            let source = format!(
                "const D: {right} = 2\nfn total(x: {left}) -> {destination} = x {operator} D\nfn main!() -> Unit = ()\n"
            );
            let compiled = compile(&source);
            assert_eq!(compiled.trace().missing().count(), 0);
            let diagnostics = compiled.diagnostics().diagnostics();
            assert_eq!(diagnostics.len(), 1, "{source}\n{diagnostics:?}");
            assert_eq!(
                diagnostics[0].code().as_str(),
                "NUMERIC_CHECKED_CONTEXT_REQUIRED"
            );
        }
    }
}

#[test]
fn remainder_records_conversion_overflow_in_its_control_flow() {
    let compiled = compile(
        "fn rem(a: u8, b: i8) -> Res[i8, NumericError] = checked { yield a % b }\nfn main!() -> Res[i8, NumericError] = rem(255, 1)\n",
    );
    let facts = compiled
        .checked_program()
        .unwrap()
        .resolution()
        .body_types();
    let operations = facts.checked_numeric_operations().collect::<Vec<_>>();
    assert_eq!(operations.len(), 1);
    assert_eq!(
        operations[0].1.operator(),
        CheckedNumericOperator::Remainder
    );
    assert!(
        operations[0]
            .1
            .faults()
            .contains(&NumericFaultCause::Overflow)
    );
    assert!(facts.transfers().iter().any(|edge| matches!(
        edge.kind(),
        ControlTransferKind::CheckedFault(NumericFaultCause::Overflow)
    )));
}

#[test]
fn successful_conversion_does_not_defer_result_overflow_to_a_wider_type() {
    let source = "fn calc(a: u8, b: i8) -> Res[i8, NumericError] = checked { yield a + b }\nfn main!() -> Res[i8, NumericError] = calc(100, 100)\n";
    let result = run(source);
    assert_eq!(result.process_status(), 1);
    assert_eq!(result.value_diagnostics()[0].code(), "INT_OVERFLOW");
    assert_eq!(result.value_diagnostics()[0].data()["dtype"].as_ref(), "i8");
}

#[test]
fn uncontextualized_literal_pairs_and_generic_calls_still_default() {
    for expression in ["1 + (2)", "1 + id(2)", "(1) + ((2))", "1 + -(2)"] {
        let source = format!(
            "fn id[T](x: T) -> T = x\nfn main!() -> Res[i64, NumericError] {{ let value = checked {{ yield {expression} }}\nyield value }}\n"
        );
        assert_integer(
            &run(&source),
            PrimitiveType::I64,
            if expression == "1 + -(2)" { -1 } else { 3 },
        );
    }
}

#[test]
fn opposite_context_reaches_literal_only_subexpressions() {
    for expression in ["(1 + 2) + x", "(1 * (1 + 2)) + x", "x + (1 + 2)"] {
        let source = format!(
            "fn calc(x: u8) -> Res[u8, NumericError] {{ let value = checked {{ yield {expression} }}\nyield value }}\nfn main!() -> Res[u8, NumericError] = calc(2)\n"
        );
        assert_integer(&run(&source), PrimitiveType::U8, 5);
    }
    let source = "fn calc(x: i8) -> Res[i8, NumericError] { let value = checked { yield (127 + 1) + x }\nyield value }\nfn main!() -> Unit = ()\n";
    let compiled = compile(source);
    let diagnostics = compiled.diagnostics().diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code().as_str(), "INT_OVERFLOW");
    assert_eq!(diagnostics[0].data()["dtype"], "i8".into());
}

#[test]
fn opposite_operand_types_literals_without_revising_completed_bindings() {
    for expression in ["1 + x", "x + 1", "(1) + x", "1 + id(x)", "(1) + (x + 1)"] {
        let source = format!(
            "fn id(x: u8) -> u8 = x\nfn calc(x: u8) -> Res[u8, NumericError] {{ let value = checked {{ yield {expression} }}\nyield value }}\nfn main!() -> Res[u8, NumericError] = calc(2)\n"
        );
        let expected = if expression == "(1) + (x + 1)" { 4 } else { 3 };
        assert_integer(&run(&source), PrimitiveType::U8, expected);
    }
    let source = "fn calc(x: u8) -> Res[i64, NumericError] { let one = 1\nyield checked { yield one + x } }\nfn main!() -> Res[i64, NumericError] = calc(2)\n";
    assert_integer(&run(source), PrimitiveType::I64, 3);
    let source = "fn calc(x: u8) -> Res[u8, NumericError] { let value = checked { yield 256 + x }\nyield value }\nfn main!() -> Unit = ()\n";
    let compiled = compile(source);
    assert!(
        compiled
            .diagnostics()
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code().as_str() == "INT_OVERFLOW")
    );
}

#[test]
fn literal_probes_keep_effects_once_and_numeric_faults_skip_later_calls() {
    let source = "import std::test::emit!\nfn small!() -> u8 { emit!(\"small\")\nyield 1 }\nfn signed!() -> i8 { emit!(\"signed\")\nyield 127 }\nfn later!() -> i8 { emit!(\"later\")\nyield 0 }\nfn main!() -> Res[i8, NumericError] { let value = checked { yield 1 + small!() }\nyield checked { yield signed!() + small!() + later!() } }\n";
    let result = run(source);
    assert_eq!(result.process_status(), 1);
    assert_eq!(result.value_diagnostics()[0].code(), "INT_OVERFLOW");
    assert_eq!(
        result.events(),
        [
            RuntimeEvent::Emitted("small".into()),
            RuntimeEvent::Emitted("signed".into()),
            RuntimeEvent::Emitted("small".into())
        ]
    );
}

#[test]
fn right_nested_literal_context_probes_use_the_worklist() {
    let expression = format!("{}x{}", "1 + (".repeat(64), ")".repeat(64));
    let source = format!(
        "fn calc(x: u16) -> Res[u16, NumericError] {{ let value = checked {{ yield {expression} }}\nyield value }}\nfn main!() -> Res[u16, NumericError] = calc(1)\n"
    );
    assert_integer(&run(&source), PrimitiveType::U16, 65);

    let expression = format!("{}missing{}", "1 + (".repeat(64), ")".repeat(64));
    let source = format!(
        "fn main!() -> Unit {{ let value = checked {{ yield {expression} }}\nyield () }}\n"
    );
    let compiled = compile(&source);
    assert!(compiled.checked_program().is_none());
    assert_eq!(compiled.trace().missing().count(), 0);
    let diagnostics = compiled.diagnostics().diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code().as_str(), "NAME_NOT_FOUND");
}
