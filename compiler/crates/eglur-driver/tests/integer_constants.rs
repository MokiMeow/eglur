use eglur_driver::{
    CompilerSession, CompletedExecution, EvaluationLimits, ExecutionAttempt, ExecutionCapability,
    ExecutionOutcome, FileSetRequest, ProgramMode, SealedCompilation, SessionOptions,
    SupportProfile, SupportProfileId, TrustedCatalog, Value, execute,
};
use eglur_sema::{CompileCapability, TypecheckCapability};

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

fn run(source: &str, profile: &SupportProfile) -> CompletedExecution {
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
        profile,
        &TrustedCatalog::first_slice().unwrap(),
        EvaluationLimits::default(),
    ) {
        ExecutionAttempt::Executed(ExecutionOutcome::Completed(result)) => result,
        other => panic!("{source}\n{other:?}"),
    }
}

fn value(expression: &str, dtype: &str, expected: &str) {
    let source = format!(
        "fn main!() -> Res[Str, Str] {{ let value: {dtype} = {expression}\nyield Ok(to_str(value)) }}\n"
    );
    let result = run(&source, &SupportProfile::semantic_core_v1());
    assert_eq!(result.process_status(), 0);
    let Value::Variant { payload, .. } = result.value() else {
        panic!("expected Ok");
    };
    assert_eq!(payload.as_ref(), [Value::Str(expected.into())], "{source}");
}

#[test]
fn integer_constant_expressions_execute_their_exact_checked_values() {
    for (expression, dtype, expected) in [
        ("1 + 2 * 3", "i8", "7"),
        ("(-7) / 3", "i8", "-2"),
        ("(-7) % 3", "i8", "-1"),
        ("7 % -3", "i8", "1"),
        ("(-128) % -1", "i8", "0"),
        ("-(127)", "i8", "-127"),
        ("-(1)", "i8", "-1"),
        ("(-1) - (-1)", "i8", "0"),
        ("18446744073709551615 / 1", "u64", "18446744073709551615"),
        ("9223372036854775807 + 0", "i64", "9223372036854775807"),
    ] {
        value(expression, dtype, expected);
    }
}

#[test]
fn constant_faults_remain_compile_errors_inside_and_outside_checked() {
    for (expression, dtype, code, operation) in [
        ("127 + 1", "i8", "INT_OVERFLOW", "add"),
        ("(-128) / -1", "i8", "INT_OVERFLOW", "divide"),
        ("3 / 0", "i8", "INT_DIV_BY_ZERO", "divide"),
        ("3 % 0", "i8", "INT_REM_BY_ZERO", "remainder"),
        ("-(-128)", "i8", "INT_OVERFLOW", "negate"),
        ("-(128)", "i8", "INT_OVERFLOW", "literal"),
        (
            "18446744073709551615 * 18446744073709551615",
            "u64",
            "INT_OVERFLOW",
            "multiply",
        ),
    ] {
        for checked in [false, true] {
            let initializer = if checked {
                format!("checked {{ yield {expression} }}")
            } else {
                expression.into()
            };
            let annotation = if checked {
                format!("Res[{dtype}, NumericError]")
            } else {
                dtype.into()
            };
            let source = format!(
                "fn main!() -> Unit {{ let value: {annotation} = {initializer}\nyield () }}\n"
            );
            let compiled = compile(&source);
            assert!(compiled.checked_program().is_none());
            assert_eq!(compiled.trace().missing().count(), 0, "{source}");
            let diagnostics = compiled.diagnostics().diagnostics();
            assert_eq!(diagnostics.len(), 1, "{source}\n{diagnostics:?}");
            assert_eq!(diagnostics[0].code().as_str(), code, "{source}");
            assert_eq!(diagnostics[0].data()["operation"], operation.into());
            assert_eq!(diagnostics[0].data()["dtype"], dtype.into());
        }
    }
}

#[test]
fn mixed_runtime_and_constant_operands_keep_runtime_faults() {
    let source = concat!(
        "fn add(x: i8) -> Res[i8, NumericError] = checked { yield x + (1 * 2) }\n",
        "fn main!() -> Res[Str, NumericError] = result { let value = add(126)?\nyield Ok(to_str(value)) }\n",
    );
    let result = run(source, &SupportProfile::semantic_core_v1());
    assert_eq!(result.process_status(), 1);
    assert_eq!(result.value_diagnostics().len(), 1);
    assert_eq!(result.value_diagnostics()[0].code(), "INT_OVERFLOW");
    assert_eq!(
        result.value_diagnostics()[0].data()["operation"].as_ref(),
        "add"
    );
}

#[test]
fn only_specified_total_integer_forms_work_without_checked() {
    for (dtype, argument, expression, expected) in [
        ("i8", "-128", "x / 2", "-64"),
        ("i8", "-128", "x % -1", "0"),
        (
            "u64",
            "18446744073709551615",
            "x / 3",
            "6148914691236517205",
        ),
    ] {
        let source = format!(
            "fn total(x: {dtype}) -> {dtype} = {expression}\nfn main!() -> Res[Str, Str] = Ok(to_str(total({argument})))\n"
        );
        let result = run(&source, &SupportProfile::semantic_core_v1());
        assert_eq!(result.process_status(), 0);
        let Value::Variant { payload, .. } = result.value() else {
            panic!("expected Ok");
        };
        assert_eq!(payload.as_ref(), [Value::Str(expected.into())]);
    }
    for expression in ["x + 0", "x * 0", "x / 0", "x / -1", "x % 0"] {
        let source = format!("fn invalid(x: i8) -> i8 = {expression}\nfn main!() -> Unit = ()\n");
        let compiled = compile(&source);
        assert_eq!(
            compiled
                .diagnostics()
                .diagnostics()
                .iter()
                .map(|d| d.code().as_str())
                .collect::<Vec<_>>(),
            ["NUMERIC_CHECKED_CONTEXT_REQUIRED"],
            "{source}"
        );
    }
}

#[test]
fn folded_arithmetic_needs_no_runtime_integer_arithmetic_capability() {
    let core = SupportProfile::semantic_core_v1();
    let profile = SupportProfile::new(
        core.schema(),
        SupportProfileId::new("folded-integer-values").unwrap(),
        core.compile_capabilities(),
        core.execution_capabilities()
            .filter(|capability| *capability != ExecutionCapability::CheckedIntegerArithmetic),
    )
    .unwrap();
    let result = run(
        "fn main!() -> Res[Str, Str] = Ok(to_str(1 + 2))\n",
        &profile,
    );
    assert_eq!(result.process_status(), 0);
}

#[test]
fn unsupported_const_references_are_not_confused_with_dynamic_values() {
    let compiled =
        compile("const ONE: f64 = 1.0\nfn main!() -> Unit { let x: f64 = -ONE\nyield () }\n");
    assert!(compiled.diagnostics().diagnostics().is_empty());
    assert!(compiled.trace().missing().any(|entry| entry.capability()
        == CompileCapability::Typecheck(TypecheckCapability::EvaluateScalarConstExpressions)));
}

#[test]
fn folded_values_do_not_elide_effects_or_mutable_reads() {
    let source = concat!(
        "import std::test::emit!\n",
        "fn make!() -> i8 { emit!(\"call\")\nyield 1 + 2 }\n",
        "fn main!() -> Res[i8, NumericError] {\n",
        "  let mut x: i8 = 1\n",
        "  x = 2\n",
        "  yield checked {\n",
        "    yield make!() + { emit!(\"block\")\nyield 1 + 2 } + x\n",
        "  }\n",
        "}\n",
    );
    let result = run(source, &SupportProfile::semantic_core_v1());
    assert_eq!(result.process_status(), 0);
    assert_eq!(
        result.events(),
        [
            eglur_driver::RuntimeEvent::Emitted("call".into()),
            eglur_driver::RuntimeEvent::Emitted("block".into()),
        ]
    );
    let Value::Variant { payload, .. } = result.value() else {
        panic!("expected Ok");
    };
    let [Value::Integer(value)] = payload.as_ref() else {
        panic!("expected integer");
    };
    assert_eq!(value.value(), 8);
}

#[test]
fn a_successful_constant_inside_checked_still_wraps_the_result() {
    let result = run(
        "fn main!() -> Res[i8, NumericError] = checked { yield -(1) + 2 }\n",
        &SupportProfile::semantic_core_v1(),
    );
    assert_eq!(result.process_status(), 0);
    let Value::Variant { payload, .. } = result.value() else {
        panic!("expected Ok");
    };
    let [Value::Integer(value)] = payload.as_ref() else {
        panic!("expected integer");
    };
    assert_eq!(value.value(), 1);
    assert!(result.value_diagnostics().is_empty());
}

#[test]
fn folded_and_proven_total_operations_have_no_checked_fault_edges() {
    let compiled = compile(concat!(
        "fn total(x: i8) -> i8 = x % -1\n",
        "fn main!() -> Res[i8, NumericError] = checked { yield 1 + 2 }\n",
    ));
    let program = compiled.checked_program().unwrap();
    let facts = program.resolution().body_types();
    let operations = facts.checked_numeric_operations().collect::<Vec<_>>();
    assert_eq!(operations.len(), 1);
    assert_eq!(
        operations[0].1.operator(),
        eglur_sema::CheckedNumericOperator::Remainder
    );
    assert!(operations[0].1.faults().is_empty());
    assert!(!facts.transfers().iter().any(|edge| matches!(
        edge.kind(),
        eglur_sema::ControlTransferKind::CheckedFault(_)
    )));
}

#[test]
fn nested_constant_chains_remain_bounded_and_exact() {
    value(&vec!["1"; 100].join(" + "), "i64", "100");
    value(
        &format!("{}1{}", "(".repeat(100), ")".repeat(100)),
        "i8",
        "1",
    );
    value(
        &format!("{}1{}", "1 + (".repeat(64), ")".repeat(64)),
        "i64",
        "65",
    );
}
