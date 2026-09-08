use eglur_driver::{
    CompilerSession, CompletedExecution, EvaluationLimits, ExecutionAttempt, ExecutionOutcome,
    FileSetRequest, ProgramMode, RuntimeEvent, SessionOptions, SupportProfile, TrustedCatalog,
    Value, execute,
};

fn run(source: &str) -> CompletedExecution {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let profile = SupportProfile::semantic_core_v1();
    let catalog = TrustedCatalog::first_slice().unwrap();
    session
        .resolve_for(ProgramMode::Executable, &profile, Some(&catalog))
        .unwrap();
    let compiled = session.seal(None).unwrap();
    let program = compiled
        .checked_program()
        .unwrap_or_else(|| panic!("{:?}\n{:?}", compiled.diagnostics(), compiled.trace()));
    match execute(program, &profile, &catalog, EvaluationLimits::default()) {
        ExecutionAttempt::Executed(ExecutionOutcome::Completed(result)) => result,
        other => panic!("expected execution: {other:?}"),
    }
}

fn integer(result: &CompletedExecution) -> i128 {
    assert_eq!(result.process_status(), 0);
    let Value::Variant { payload, .. } = result.value() else {
        panic!("expected Ok")
    };
    let [Value::Integer(value)] = payload.as_ref() else {
        panic!("expected integer")
    };
    value.value()
}

#[test]
fn long_checked_integer_chains_execute_on_the_default_stack() {
    for expression in [
        vec!["x"; 100].join(" + "),
        format!("{}x{}", "x + (".repeat(64), ")".repeat(64)),
    ] {
        let source = format!(
            "fn sum(x: i64) -> Res[i64, NumericError] = checked {{ yield {expression} }}\nfn main!() -> Res[i64, NumericError] = sum(1)\n"
        );
        let expected = expression.bytes().filter(|&byte| byte == b'x').count() as i128;
        assert_eq!(integer(&run(&source)), expected);
    }
}

#[test]
fn calls_keep_source_order_and_a_numeric_fault_skips_later_effects() {
    let source = concat!(
        "import std::test::emit!\n",
        "fn left!() -> i8 { emit!(\"left\")\nyield 127 }\n",
        "fn right!() -> i8 { emit!(\"right\")\nyield 1 }\n",
        "fn later!() -> i8 { emit!(\"later\")\nyield 2 }\n",
        "fn main!() -> Res[i8, NumericError] = checked { yield left!() + right!() + later!() }\n",
    );
    let result = run(source);
    assert_eq!(result.process_status(), 1);
    assert_eq!(
        result.events(),
        [
            RuntimeEvent::Emitted("left".into()),
            RuntimeEvent::Emitted("right".into())
        ]
    );
    assert_eq!(result.value_diagnostics().len(), 1);
    assert_eq!(result.value_diagnostics()[0].code(), "INT_OVERFLOW");
}

#[test]
fn nested_call_arguments_and_block_operands_keep_their_typing_boundaries() {
    let constants = vec!["1"; 100].join(" + ");
    let calls = vec!["id!(1)"; 64].join(" + ");
    let source = format!(
        "import std::test::emit!\nfn id!(x: i64) -> i64 {{ emit!(\"id\")\nyield x }}\nfn main!() -> Res[i64, NumericError] = checked {{ yield id!({constants}) + ({{ let x: i64 = 2\nyield x }}) + ({calls}) }}\n"
    );
    let result = run(&source);
    assert_eq!(integer(&result), 166);
    assert_eq!(
        result.events(),
        vec![RuntimeEvent::Emitted("id".into()); 65]
    );
}
