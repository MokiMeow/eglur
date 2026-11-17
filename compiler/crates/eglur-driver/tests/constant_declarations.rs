use eglur_driver::{
    CompilerSession, CompletedExecution, EvaluationLimits, ExecutionAttempt, ExecutionOutcome,
    FileInput, FileSetRequest, ProgramMode, SealedCompilation, SessionOptions, SupportProfile,
    TrustedCatalog, Value, execute,
};

fn compile(files: &[(&str, &str)]) -> SealedCompilation {
    let request = FileSetRequest::new(
        files
            .iter()
            .map(|(path, source)| FileInput::new(*path, source.as_bytes().to_vec())),
        "main.eglur",
        (files.len() > 1).then_some("."),
    )
    .unwrap();
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

fn run(files: &[(&str, &str)]) -> CompletedExecution {
    let compiled = compile(files);
    let program = compiled
        .checked_program()
        .unwrap_or_else(|| panic!("{:?}\n{:?}", compiled.diagnostics(), compiled.trace()));
    match execute(
        program,
        &SupportProfile::semantic_core_v1(),
        &TrustedCatalog::first_slice().unwrap(),
        EvaluationLimits::default(),
    ) {
        ExecutionAttempt::Executed(ExecutionOutcome::Completed(result)) => result,
        other => panic!("{other:?}"),
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
fn source_constants_are_evaluated_before_function_consumers() {
    let result = run(&[(
        "main.eglur",
        concat!(
            "fn value() -> i8 = TOTAL\n",
            "const BASE: i8 = 6 * 7\n",
            "const TOTAL: i8 = BASE + 1\n",
            "fn main!() -> Res[i8, Str] = Ok(value())\n",
        ),
    )]);
    assert_eq!(integer(&result), 43);
}

#[test]
fn imported_and_reexported_diamonds_keep_canonical_identity() {
    let files = [
        (
            "main.eglur",
            "import left::LEFT\nimport right::RIGHT\nconst TOTAL: i8 = LEFT + RIGHT\nfn main!() -> Res[i8, Str] = Ok(TOTAL)\n",
        ),
        (
            "left.eglur",
            "import root::BASE as START\npub const LEFT: i8 = START + 1\n",
        ),
        (
            "right.eglur",
            "import relay::BASE\npub const RIGHT: i8 = BASE + 2\n",
        ),
        ("relay.eglur", "pub import root::BASE\n"),
        ("root.eglur", "pub const BASE: i8 = 20\n"),
    ];
    assert_eq!(integer(&run(&files)), 43);
    let reversed = files.into_iter().rev().collect::<Vec<_>>();
    assert_eq!(integer(&run(&reversed)), 43);
}

#[test]
fn cycle_diagnostics_precede_forward_references_and_propagate() {
    for source in [
        "const A: i8 = B\nconst B: i8 = A\nconst C: i8 = A\nfn main!() -> Res[i8, Str] = Ok(C)\n",
        "const A: i8 = A\nfn main!() -> Res[i8, Str] = Ok(A)\n",
    ] {
        let compiled = compile(&[("main.eglur", source)]);
        let diagnostics = compiled.diagnostics().diagnostics();
        assert!(!diagnostics.is_empty());
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code().as_str() == "CONST_CYCLE")
        );
        assert!(compiled.checked_program().is_none());
        assert_eq!(compiled.trace().missing().count(), 0);
    }
    let compiled = compile(&[
        (
            "main.eglur",
            "import other::B\npub const A: i8 = B\nfn main!() -> Res[i8, Str] = Ok(A)\n",
        ),
        ("other.eglur", "import main::A\npub const B: i8 = A\n"),
    ]);
    assert_eq!(compiled.diagnostics().diagnostics().len(), 2);
    assert!(
        compiled
            .diagnostics()
            .diagnostics()
            .iter()
            .all(|d| d.code().as_str() == "CONST_CYCLE")
    );
}

#[test]
fn unrelated_initializer_errors_do_not_hide_forward_references() {
    let compiled = compile(&[(
        "main.eglur",
        "const A: i8 = B\nconst B: i8 = 127 + 1\nfn main!() -> Unit = ()\n",
    )]);
    assert_eq!(
        compiled
            .diagnostics()
            .diagnostics()
            .iter()
            .map(|d| d.code().as_str())
            .collect::<Vec<_>>(),
        ["CONST_FORWARD_REFERENCE", "INT_OVERFLOW"]
    );
    let compiled = compile(&[(
        "main.eglur",
        "const A: i8 = B\nconst B: i8 = A + (127 + 1)\nconst C: i8 = A\nfn main!() -> Unit = ()\n",
    )]);
    let diagnostics = compiled.diagnostics().diagnostics();
    assert_eq!(diagnostics.len(), 2);
    assert!(
        diagnostics
            .iter()
            .all(|d| d.code().as_str() == "CONST_CYCLE")
    );
}

#[test]
fn forward_references_faults_and_nonconstant_initializers_are_compile_errors() {
    for (declarations, code) in [
        (
            "const A: i8 = B\nconst B: i8 = 1\n",
            "CONST_FORWARD_REFERENCE",
        ),
        ("const A: i8 = 127 + 1\nconst B: i8 = A\n", "INT_OVERFLOW"),
        ("const A: i8 = 1 / 0\n", "INT_DIV_BY_ZERO"),
        ("const A: i8 = missing\n", "NAME_NOT_FOUND"),
        (
            "fn f() -> i8 = 1\nconst A: i8 = f()\n",
            "CONST_INIT_NOT_CONST",
        ),
        (
            "fn f!() -> i8 = 1\nconst A: i8 = f!()\n",
            "CONST_INIT_NOT_CONST",
        ),
        ("const A: i8 = true\n", "ASSIGNMENT_TYPE_MISMATCH"),
    ] {
        let source = format!("{declarations}fn main!() -> Res[i8, Str] = Ok(A)\n");
        let compiled = compile(&[("main.eglur", &source)]);
        let diagnostics = compiled.diagnostics().diagnostics();
        assert_eq!(
            diagnostics
                .iter()
                .map(|d| d.code().as_str())
                .collect::<Vec<_>>(),
            [code],
            "{source}\n{diagnostics:?}"
        );
        assert_eq!(compiled.trace().missing().count(), 0, "{source}");
        assert!(compiled.checked_program().is_none());
    }
}

#[test]
fn local_shadowing_does_not_propagate_constant_values() {
    let result = run(&[(
        "main.eglur",
        concat!(
            "const VALUE: i8 = 1\n",
            "fn main!() -> Res[i8, NumericError] { let mut VALUE: i8 = 2\nVALUE = 3\nyield checked { yield VALUE + 4 } }\n",
        ),
    )]);
    assert_eq!(integer(&result), 7);
}

#[test]
fn named_operands_supply_literal_types_without_recursive_dependency_expansion() {
    let nested = format!("{}BASE{}", "BASE + (".repeat(64), ")".repeat(64));
    let source = format!(
        "type Scalar = i16\nconst BASE: Scalar = 1\nfn main!() -> Res[i16, Str] {{ let value = {nested} + 1\nyield Ok(value) }}\n"
    );
    assert_eq!(integer(&run(&[("main.eglur", &source)])), 66);

    let mut source = String::from("const C0: i16 = 0\n");
    for index in 1..256 {
        source.push_str(&format!("const C{index}: i16 = C{} + 1\n", index - 1));
    }
    source.push_str("fn main!() -> Res[i16, Str] = Ok(C255)\n");
    assert_eq!(integer(&run(&[("main.eglur", &source)])), 255);
}

#[test]
fn unavailable_constant_declarations_remain_an_explicit_profile_gap() {
    use eglur_driver::SupportProfileId;
    use eglur_sema::{CompileCapability, TypecheckCapability};

    let core = SupportProfile::semantic_core_v1();
    let capability =
        CompileCapability::Typecheck(TypecheckCapability::EvaluateSelectedIntegerConstDeclarations);
    let profile = SupportProfile::new(
        core.schema(),
        SupportProfileId::new("without-const-declarations").unwrap(),
        core.compile_capabilities()
            .filter(|&candidate| candidate != capability),
        core.execution_capabilities(),
    )
    .unwrap();
    let mut session = CompilerSession::build(
        FileSetRequest::single(
            "main.eglur",
            b"const VALUE: i8 = 1\nfn main!() -> Res[i8, Str] = Ok(VALUE)\n".to_vec(),
        )
        .unwrap(),
        SessionOptions::default(),
    )
    .unwrap();
    session
        .resolve_for(
            ProgramMode::Executable,
            &profile,
            Some(&TrustedCatalog::first_slice().unwrap()),
        )
        .unwrap();
    let compiled = session.seal(None).unwrap();
    assert!(compiled.diagnostics().diagnostics().is_empty());
    assert!(compiled.checked_program().is_none());
    assert!(
        compiled
            .trace()
            .missing()
            .any(|entry| entry.capability() == capability)
    );
}

#[test]
fn noninteger_constants_remain_an_explicit_capability_gap() {
    let compiled = compile(&[(
        "main.eglur",
        "const VALUE: f64 = 1.0\nfn main!() -> Unit = ()\n",
    )]);
    assert!(compiled.diagnostics().diagnostics().is_empty());
    assert!(compiled.checked_program().is_none());
    assert!(
        compiled
            .trace()
            .missing()
            .any(|entry| entry.capability().stable_key() == "typecheck.scalar_const_evaluation")
    );
}

#[test]
fn unimplemented_initializer_forms_are_not_false_language_errors() {
    let compiled = compile(&[(
        "main.eglur",
        "const VALUE: i8 = { yield 1 }\nfn main!() -> Unit = ()\n",
    )]);
    assert!(compiled.diagnostics().diagnostics().is_empty());
    assert!(compiled.checked_program().is_none());
    assert!(
        compiled
            .trace()
            .missing()
            .any(|entry| entry.capability().stable_key() == "typecheck.scalar_const_evaluation")
    );
}

#[test]
fn imported_constant_changes_invalidate_consumer_body_fingerprints() {
    use eglur_sema::ModuleAddress;
    let main = "import values::VALUE\nfn main!() -> Res[i8, Str] = Ok(VALUE)\n";
    let before = compile(&[
        ("main.eglur", main),
        ("values.eglur", "pub const VALUE: i8 = 1\n"),
    ]);
    let after = compile(&[
        ("main.eglur", main),
        ("values.eglur", "pub const VALUE: i8 = 2\n"),
    ]);
    let before = before
        .checked_program()
        .unwrap()
        .resolution()
        .fingerprints()
        .unwrap();
    let after = after
        .checked_program()
        .unwrap()
        .resolution()
        .fingerprints()
        .unwrap();
    let producer = ModuleAddress::new([Box::<str>::from("values")]).unwrap();
    let consumer = ModuleAddress::new([Box::<str>::from("main")]).unwrap();
    assert_ne!(
        before.module(&producer).unwrap().interface(),
        after.module(&producer).unwrap().interface()
    );
    assert_ne!(
        before.module(&consumer).unwrap().body(),
        after.module(&consumer).unwrap().body()
    );
}
