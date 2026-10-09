use eglur_driver::{
    CompileAttempt, CompileAttemptError, CompileDisposition, CompilerSession, FileInput,
    FileSetRequest, ProgramMode, SessionOptions, SupportProfile, TrustedCatalog,
};
use serde_json::Value;

fn attempt(files: &[(&str, &str)]) -> CompileAttempt {
    let request = FileSetRequest::new(
        files
            .iter()
            .map(|(path, text)| FileInput::new(*path, text.as_bytes().to_vec())),
        "main.eglur",
        (files.len() > 1).then_some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    session
        .resolve_for(
            ProgramMode::Library,
            &SupportProfile::semantic_core_v1(),
            Some(&catalog),
        )
        .unwrap();
    let sealed = session.seal(None).unwrap();
    if sealed.trace().has_missing() {
        assert!(sealed.checked_program().is_none());
    }
    CompileAttempt::finalize(sealed, None).unwrap()
}

fn assert_missing(attempt: &CompileAttempt, expected: &[&str]) {
    assert_eq!(attempt.disposition(), CompileDisposition::Unsupported);
    assert!(matches!(
        attempt.compiler_result(),
        Err(CompileAttemptError::Inconclusive(
            CompileDisposition::Unsupported
        ))
    ));
    let payload: Value =
        serde_json::from_str(&attempt.development_attempt().unwrap().to_json()).unwrap();
    assert!(
        payload["diagnostics"].as_array().unwrap().is_empty(),
        "{payload}"
    );
    let missing = payload["capability_trace"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["disposition"] == "missing")
        .map(|entry| entry["capability"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(missing, expected, "{payload}");
    assert_eq!(
        attempt.render_human(),
        format!("tool unsupported: {}", expected.join(", "))
    );
}

#[test]
fn trusted_qualified_calls_and_values_report_missing_inherent_resolution() {
    for source in [
        "fn source(value: &NumericError) -> Unit = NumericError.missing(value)\n",
        "fn source(value: &NumericError) -> Unit = ((NumericError.missing))(value)\n",
        "fn source() -> Unit { let f: Fn(&NumericError) -> Unit = NumericError.missing\n_ = f\nyield () }\n",
        "fn source() -> Unit { let f: Fn(&NumericError) -> Unit = ((NumericError.missing))\n_ = f\nyield () }\n",
        "type ErrorAlias = NumericError\nfn source(value: &ErrorAlias) -> Unit = ErrorAlias.missing(value)\n",
        "type ErrorAlias = NumericError\nfn source() -> Unit { let f: Fn(&ErrorAlias) -> Unit = ErrorAlias.missing\n_ = f\nyield () }\n",
        "import std::io::IoError as ErrorAlias\nfn source(value: &ErrorAlias) -> Unit = ErrorAlias.missing(value)\n",
        "import std::io::IoError as ErrorAlias\nfn source() -> Unit { let f: Fn(&ErrorAlias) -> Unit = ErrorAlias.missing\n_ = f\nyield () }\n",
    ] {
        let result = attempt(&[("main.eglur", source)]);
        assert_missing(&result, &["typecheck.trusted_inherent_methods"]);
    }
}

#[test]
fn trusted_owner_reexports_preserve_the_missing_inherent_operation() {
    for body in [
        "fn source(value: &PublicError) -> Unit = PublicError.missing(value)\n",
        "fn source() -> Unit { let f: Fn(&PublicError) -> Unit = PublicError.missing\n_ = f\nyield () }\n",
    ] {
        let source = format!("import bridge::PublicError\n{body}");
        let result = attempt(&[
            ("main.eglur", &source),
            (
                "bridge.eglur",
                "pub import std::io::IoError as PublicError\n",
            ),
        ]);
        assert_missing(&result, &["typecheck.trusted_inherent_methods"]);
    }
}

#[test]
fn genuine_closures_remain_distinct_from_trusted_member_refusals() {
    let closure = "fn closure() -> Unit { let f: Fn() -> Unit = { || () }\n_ = f\nyield () }\n";
    assert_missing(
        &attempt(&[("main.eglur", closure)]),
        &["typecheck.closures"],
    );

    let source =
        format!("fn source(value: &NumericError) -> Unit = NumericError.missing(value)\n{closure}");
    assert_missing(
        &attempt(&[("main.eglur", &source)]),
        &["typecheck.trusted_inherent_methods", "typecheck.closures"],
    );
}

#[test]
fn source_and_trusted_trait_methods_still_compile_without_missing_support() {
    for source in [
        "struct Count { fn read(self: &Self) -> i64 = 7 }\nfn source(value: &Count) -> i64 = Count.read(value)\n",
        "struct Count { fn read(self: &Self) -> i64 = 7 }\nfn source() -> Unit { let f: Fn(&Count) -> i64 = Count.read\n_ = f\nyield () }\n",
        "fn source(value: &Bool) -> Str = Display.to_str(value)\n",
        "fn source() -> Unit { let f: Fn(&Bool) -> Str = Display.to_str\n_ = f\nyield () }\n",
    ] {
        let result = attempt(&[("main.eglur", source)]);
        assert_eq!(
            result.disposition(),
            CompileDisposition::Succeeded,
            "{source}\n{}",
            result.render_human()
        );
        assert!(result.compiler_result().unwrap().succeeded());
        assert!(result.development_attempt().is_none());
    }
}
