use eglur_driver::{
    CompilerSession, EvaluationLimits, ExecutionAttempt, ExecutionOutcome, FileSetRequest,
    ProgramMode, SealedCompilation, SessionOptions, SupportProfile, TrustedCatalog, Value, execute,
};

fn compile(source: &str) -> SealedCompilation {
    compile_mode(source, ProgramMode::Library)
}

fn compile_mode(source: &str, mode: ProgramMode) -> SealedCompilation {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve_for(mode, &SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let missing = analysis
        .trace()
        .missing()
        .map(|demand| demand.capability().stable_key())
        .collect::<Vec<_>>();
    assert!(missing.is_empty(), "{source}\nmissing: {missing:?}");
    session.seal(None).unwrap()
}

fn value(expression: &str, dtype: &str, expected: &str) {
    let source = format!(
        "fn main!() -> Res[Str, Str] {{ let value: {dtype} = {expression}\nyield Ok(to_str(value)) }}\n"
    );
    let compiled = compile_mode(&source, ProgramMode::Executable);
    let program = compiled
        .checked_program()
        .unwrap_or_else(|| panic!("{source}\n{:?}", compiled.diagnostics()));
    let attempt = execute(
        program,
        &SupportProfile::semantic_core_v1(),
        &TrustedCatalog::first_slice().unwrap(),
        EvaluationLimits::default(),
    );
    let ExecutionAttempt::Executed(ExecutionOutcome::Completed(result)) = attempt else {
        panic!("{attempt:?}");
    };
    let Value::Variant { payload, .. } = result.value() else {
        panic!("expected Ok");
    };
    assert_eq!(payload.as_ref(), [Value::Str(expected.into())], "{source}");
}

fn error(source: &str, expected: &str) {
    let compiled = compile(source);
    let codes = compiled
        .diagnostics()
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect::<Vec<_>>();
    assert_eq!(codes, [expected], "{source}\n{:?}", compiled.diagnostics());
    assert!(compiled.checked_program().is_none());
}

#[test]
fn signed_minima_and_zero_execute_without_checked_arithmetic() {
    for (dtype, minimum, beyond) in [
        ("i8", "-128", "-129"),
        ("i16", "-32768", "-32769"),
        ("i32", "-2147483648", "-2147483649"),
        ("i64", "-9223372036854775808", "-9223372036854775809"),
    ] {
        value(minimum, dtype, minimum);
        value("-0", dtype, "0");
        error(
            &format!("fn main!() -> Unit {{ let value: {dtype} = {beyond}\nyield () }}\n"),
            "INT_OVERFLOW",
        );
    }
    value("- /* sign */ 000_128", "i8", "-128");
    value("(-128)", "i8", "-128");
}

#[test]
fn negative_domain_literals_use_the_domain_channel() {
    value("-0", "Nat", "0");
    value("-0_0", "Nat", "0");
    for (dtype, literal) in [
        ("Nat", "-1"),
        ("PositiveNat", "-1"),
        ("PositiveNat", "-0"),
        ("Nat", "-999999999999999999999999999999999999999"),
    ] {
        let source = format!("fn main!() -> Unit {{ let value: {dtype} = {literal}\nyield () }}\n");
        error(&source, "DOMAIN_INVALID_VALUE");
        let compiled = compile(&source);
        let diagnostic = &compiled.diagnostics().diagnostics()[0];
        assert_eq!(diagnostic.data().get("domain"), Some(&dtype.into()));
        assert_eq!(diagnostic.data().get("value"), Some(&literal.into()));
    }
}

#[test]
fn unsigned_negative_nonzero_values_overflow() {
    for dtype in ["u8", "u16", "u32", "u64"] {
        value("-0", dtype, "0");
        error(
            &format!("fn main!() -> Unit {{ let value: {dtype} = -1\nyield () }}\n"),
            "INT_OVERFLOW",
        );
    }
}

#[test]
fn aliases_calls_returns_and_const_declarations_share_literal_rules() {
    for source in [
        "type Small = i8\nfn take(x: Small) -> Small = x\nfn main!() -> Str = to_str(take(-128))\n",
        "fn take() -> i8 = -128\nfn main!() -> Str = to_str(take())\n",
        "const MINIMUM: i8 = -128\nfn main!() -> Unit = ()\n",
        "fn main!() -> Str { let value = -9223372036854775808\nyield to_str(value) }\n",
        "fn main!() -> Res[i8, NumericError] = checked { yield -128 }\n",
    ] {
        let compiled = compile(source);
        assert!(
            compiled.checked_program().is_some(),
            "{source}\n{:?}",
            compiled.diagnostics()
        );
    }
    error(
        "type Count = Nat\nfn take(x: Count) -> Unit = ()\nfn main!() -> Unit = take(-1)\n",
        "DOMAIN_INVALID_VALUE",
    );
    error(
        "fn main!() -> Unit { let value = -9223372036854775809\nyield () }\n",
        "INT_OVERFLOW",
    );
}

#[test]
fn signed_literals_respect_independent_generic_and_overload_contexts() {
    let compiled = compile("fn id[T](x: T) -> T = x\nfn main!() -> i8 = id::[i8](-128)\n");
    assert!(
        compiled.checked_program().is_some(),
        "{:?}",
        compiled.diagnostics()
    );
    let source = "overload fn width(x: i8) -> i64 = 8\noverload fn width(x: i16) -> i64 = 16\nfn main!() -> i64 = width(-1)\n";
    error(source, "CALL_ARG_TYPE_MISMATCH");
    let compiled = compile(source);
    assert_eq!(
        compiled.diagnostics().diagnostics()[0].data().get("reason"),
        Some(&"overload_no_match".into())
    );
    assert_eq!(
        compiled.diagnostics().diagnostics()[0]
            .data()
            .get("actual_type"),
        Some(&"(i64,)".into())
    );
}

#[test]
fn execution_carries_signed_literals_through_generic_calls() {
    let source =
        "fn id[T](x: T) -> T = x\nfn main!() -> Res[Str, Str] = Ok(to_str(id::[i8](-128)))\n";
    let compiled = compile_mode(source, ProgramMode::Executable);
    let attempt = execute(
        compiled.checked_program().unwrap(),
        &SupportProfile::semantic_core_v1(),
        &TrustedCatalog::first_slice().unwrap(),
        EvaluationLimits::default(),
    );
    let ExecutionAttempt::Executed(ExecutionOutcome::Completed(result)) = attempt else {
        panic!("{attempt:?}");
    };
    let Value::Variant { payload, .. } = result.value() else {
        panic!("expected Ok");
    };
    assert_eq!(payload.as_ref(), [Value::Str("-128".into())]);
}

#[test]
fn literal_fingerprints_keep_sign_but_ignore_intervening_trivia() {
    fn fingerprint(expression: &str) -> eglur_sema::SemanticFingerprints {
        let source = format!("fn main!() -> i8 = {expression}\n");
        compile(&source)
            .checked_program()
            .unwrap()
            .resolution()
            .fingerprints()
            .unwrap()
            .clone()
    }
    assert_eq!(fingerprint("-1"), fingerprint("- /* sign */ 1"));
    assert_ne!(fingerprint("-1"), fingerprint("1"));
}
