use eglur_driver::{
    CompilerSession, CompletedExecution, EvaluationLimits, ExecutionAttempt, ExecutionCapability,
    ExecutionOutcome, FileSetRequest, ProgramMode, RuntimeEvent, SealedCompilation, SessionOptions,
    SupportProfile, SupportProfileId, SupportReason, TrustedCatalog, Value, execute,
};
use eglur_sema::PrimitiveType;

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
    assert_eq!(result.process_status(), 0);
    assert!(result.value_diagnostics().is_empty());
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
fn wrapping_family_executes_all_width_boundaries_without_checked() {
    for dtype in [
        PrimitiveType::I8,
        PrimitiveType::I16,
        PrimitiveType::I32,
        PrimitiveType::I64,
        PrimitiveType::U8,
        PrimitiveType::U16,
        PrimitiveType::U32,
        PrimitiveType::U64,
    ] {
        let (min, max) = dtype.integer_bounds().unwrap();
        for (function, a, b, expected) in [
            ("wrapping_add", max, 1, min),
            ("wrapping_sub", min, 1, max),
            ("wrapping_mul", max, max, 1),
            ("wrapping_mul", min, min, 0),
        ] {
            let source = format!(
                "type Scalar = {}\nfn calc(a: Scalar, b: Scalar) -> Scalar = {function}(a, b)\nfn main!() -> Res[Scalar, Str] = Ok(calc({a}, {b}))\n",
                dtype.spelling()
            );
            assert_integer(&run(&source), dtype, expected);
        }
    }
}

#[test]
fn generic_literal_default_context_and_explicit_type_arguments_are_preserved() {
    for (expression, annotation, dtype, expected) in [
        ("wrapping_mul(64, 2)", "", PrimitiveType::I64, 128),
        ("wrapping_mul(64, 2)", ": i8", PrimitiveType::I8, -128),
        ("wrapping_mul::[i8](64, 2)", "", PrimitiveType::I8, -128),
        ("wrapping_sub(0, 1)", "", PrimitiveType::I64, -1),
        ("wrapping_sub(0, 1)", ": u8", PrimitiveType::U8, 255),
        ("wrapping_sub::[u8](0, 1)", "", PrimitiveType::U8, 255),
    ] {
        let source = format!(
            "fn main!() -> Res[{}, Str] {{ let value{annotation} = {expression}\nyield Ok(value) }}\n",
            dtype.spelling()
        );
        assert_integer(&run(&source), dtype, expected);
    }
}

#[test]
fn invalid_type_families_and_mixed_dtypes_are_not_promoted() {
    for function in ["wrapping_add", "wrapping_sub", "wrapping_mul"] {
        for dtype in ["Bool", "Nat", "PositiveNat", "f64", "complex64"] {
            let source = format!(
                "fn invalid(x: {dtype}) -> {dtype} = {function}(x, x)\nfn main!() -> Unit = ()\n"
            );
            let compiled = compile(&source);
            assert_eq!(compiled.trace().missing().count(), 0, "{source}");
            let diagnostics = compiled.diagnostics().diagnostics();
            assert_eq!(diagnostics.len(), 1, "{source}\n{diagnostics:?}");
            assert_eq!(diagnostics[0].code().as_str(), "CALL_ARG_TYPE_MISMATCH");
            assert_eq!(
                diagnostics[0].data()["expected_family"],
                "FixedWidthInteger".into()
            );
        }
        let source =
            format!("fn invalid(a: i8, b: u8) -> i8 = {function}(a, b)\nfn main!() -> Unit = ()\n");
        let compiled = compile(&source);
        let diagnostics = compiled.diagnostics().diagnostics();
        assert_eq!(compiled.trace().missing().count(), 0);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code().as_str(), "TYPE_ARG_INFERENCE_FAILED");
    }
}

#[test]
fn imported_helpers_keep_canonical_identity_and_source_order_effects() {
    for (function, expected) in [("wrapping_sub", 255), ("wrapping_mul", 0)] {
        let source = format!(
            "import std::prelude::{function} as wrap\nimport std::test::emit!\nfn left!() -> u8 {{ emit!(\"left\")\nyield 0 }}\nfn right!() -> u8 {{ emit!(\"right\")\nyield 1 }}\nfn main!() -> Res[u8, Str] = Ok(wrap(left!(), right!()))\n"
        );
        let result = run(&source);
        assert_integer(&result, PrimitiveType::U8, expected);
        assert_eq!(
            result.events(),
            [
                RuntimeEvent::Emitted("left".into()),
                RuntimeEvent::Emitted("right".into())
            ]
        );
    }
}

#[test]
fn missing_runtime_capability_refuses_before_execution() {
    let core = SupportProfile::semantic_core_v1();
    let profile = SupportProfile::new(
        core.schema(),
        SupportProfileId::new("wrapping-without-hooks").unwrap(),
        core.compile_capabilities(),
        core.execution_capabilities()
            .filter(|&capability| capability != ExecutionCapability::TrustedScalarHooks),
    )
    .unwrap();
    for function in ["wrapping_add", "wrapping_sub", "wrapping_mul"] {
        let source = format!(
            "import std::test::emit!\nfn main!() -> Res[u8, Str] {{ emit!(\"before\")\nyield Ok({function}::[u8](0, 1)) }}\n"
        );
        let compiled = compile(&source);
        let program = compiled.checked_program().unwrap();
        assert_eq!(
            execute(
                program,
                &profile,
                &TrustedCatalog::first_slice().unwrap(),
                EvaluationLimits::default()
            ),
            ExecutionAttempt::Unsupported(SupportReason::MissingCapability(
                ExecutionCapability::TrustedScalarHooks
            ))
        );
    }
}
