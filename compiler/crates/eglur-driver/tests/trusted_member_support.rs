use eglur_driver::{
    CompileAttempt, CompileAttemptError, CompileDisposition, CompilerSession, FileInput,
    FileSetRequest, ProgramMode, SessionOptions, SupportProfile, ToolLimit, TrustedCatalog,
};
use serde_json::Value;

fn attempt(files: &[(&str, &str)]) -> CompileAttempt {
    attempt_with_limit(files, None)
}

fn attempt_with_limit(files: &[(&str, &str)], limit: Option<ToolLimit>) -> CompileAttempt {
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
    if sealed.trace().has_unsupported() {
        assert!(sealed.checked_program().is_none());
    }
    CompileAttempt::finalize(sealed, limit).unwrap()
}

fn assert_missing(attempt: &CompileAttempt, expected: &[&str], owner: Option<&str>) {
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
    let mut labels = expected
        .iter()
        .map(|key| (*key).to_owned())
        .collect::<Vec<_>>();
    if let Some(owner) = owner {
        let entry = payload["capability_trace"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["capability"] == "typecheck.trusted_inherent_methods")
            .unwrap();
        assert_eq!(
            entry["unavailable_inputs"][0]["kind"],
            "trusted_inherent_inventory"
        );
        assert_eq!(entry["unavailable_inputs"][0]["owner"], owner);
        assert!(
            !entry["unavailable_inputs"][0]["locations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        labels[0].push_str(&format!(" (missing inherent inventory: {owner})"));
    }
    assert_eq!(
        attempt.render_human(),
        format!("tool unsupported: {}", labels.join(", "))
    );
}

#[test]
fn trusted_qualified_calls_and_values_report_missing_inherent_resolution() {
    for (source, owner) in [
        (
            "fn source(value: &NumericError) -> Unit = NumericError.missing(value)\n",
            "std::prelude::NumericError",
        ),
        (
            "fn source(value: &NumericError) -> Unit = ((NumericError.missing))(value)\n",
            "std::prelude::NumericError",
        ),
        (
            "fn source() -> Unit { let f: Fn(&NumericError) -> Unit = NumericError.missing\n_ = f\nyield () }\n",
            "std::prelude::NumericError",
        ),
        (
            "fn source() -> Unit { let f: Fn(&NumericError) -> Unit = ((NumericError.missing))\n_ = f\nyield () }\n",
            "std::prelude::NumericError",
        ),
        (
            "type ErrorAlias = NumericError\nfn source(value: &ErrorAlias) -> Unit = ErrorAlias.missing(value)\n",
            "std::prelude::NumericError",
        ),
        (
            "type ErrorAlias = NumericError\nfn source() -> Unit { let f: Fn(&ErrorAlias) -> Unit = ErrorAlias.missing\n_ = f\nyield () }\n",
            "std::prelude::NumericError",
        ),
        (
            "import std::io::IoError as ErrorAlias\nfn source(value: &ErrorAlias) -> Unit = ErrorAlias.missing(value)\n",
            "std::io::IoError",
        ),
        (
            "import std::io::IoError as ErrorAlias\nfn source() -> Unit { let f: Fn(&ErrorAlias) -> Unit = ErrorAlias.missing\n_ = f\nyield () }\n",
            "std::io::IoError",
        ),
    ] {
        let result = attempt(&[("main.eglur", source)]);
        assert_missing(
            &result,
            &["typecheck.trusted_inherent_methods"],
            Some(owner),
        );
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
        assert_missing(
            &result,
            &["typecheck.trusted_inherent_methods"],
            Some("std::io::IoError"),
        );
    }
}

#[test]
fn genuine_closures_remain_distinct_from_trusted_member_refusals() {
    let closure = "fn closure() -> Unit { let f: Fn() -> Unit = { || () }\n_ = f\nyield () }\n";
    assert_missing(
        &attempt(&[("main.eglur", closure)]),
        &["typecheck.closures"],
        None,
    );

    let source =
        format!("fn source(value: &NumericError) -> Unit = NumericError.missing(value)\n{closure}");
    assert_missing(
        &attempt(&[("main.eglur", &source)]),
        &["typecheck.trusted_inherent_methods", "typecheck.closures"],
        Some("std::prelude::NumericError"),
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

#[test]
fn multiple_owner_refusals_keep_canonical_identity_and_separate_demand_sites() {
    let result = attempt(&[(
        "main.eglur",
        "import std::io::IoError as Alias\nfn one(value: &NumericError) -> Unit = NumericError.missing(value)\nfn two() -> Unit { let f: Fn(&Alias) -> Unit = ((Alias.missing))\n_ = f\nyield () }\n",
    )]);
    let CompileAttempt::Unsupported(unsupported) = &result else {
        panic!("expected unsupported: {}", result.render_human());
    };
    let entry = unsupported.trace().trace().unsupported().next().unwrap();
    let refusals = entry.input_refusals();
    assert_eq!(refusals.len(), 2);
    assert_eq!(refusals[0].locations().len(), 1);
    assert_eq!(refusals[1].locations().len(), 1);
    assert_ne!(refusals[0].locations(), refusals[1].locations());
    let payload: Value =
        serde_json::from_str(&result.development_attempt().unwrap().to_json()).unwrap();
    let inputs = payload["capability_trace"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["capability"] == "typecheck.trusted_inherent_methods")
        .unwrap()["unavailable_inputs"]
        .as_array()
        .unwrap();
    assert_eq!(inputs[0]["owner"], "std::io::IoError");
    assert_eq!(inputs[1]["owner"], "std::prelude::NumericError");
    assert_eq!(
        result.render_human(),
        "tool unsupported: typecheck.trusted_inherent_methods (missing inherent inventory: std::io::IoError) (missing inherent inventory: std::prelude::NumericError)"
    );
}

#[test]
fn source_errors_and_tool_limits_preserve_unavailable_input_causes() {
    let source = "fn one(value: &NumericError) -> Unit = NumericError.missing(value)\nfn bad() -> Bool = 0\n";
    for limited in [false, true] {
        let result = attempt_with_limit(
            &[("main.eglur", source)],
            limited.then(|| ToolLimit::new("driver.test_limit").unwrap()),
        );
        assert_eq!(
            result.disposition(),
            if limited {
                CompileDisposition::ToolLimitExceeded
            } else {
                CompileDisposition::Unsupported
            }
        );
        assert!(result.compiler_result().is_err());
        let payload: Value =
            serde_json::from_str(&result.development_attempt().unwrap().to_json()).unwrap();
        assert!(
            payload["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == "RETURN_TYPE_MISMATCH")
        );
        let entry = payload["capability_trace"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["capability"] == "typecheck.trusted_inherent_methods")
            .unwrap();
        assert_eq!(
            entry["unavailable_inputs"][0]["owner"],
            "std::prelude::NumericError"
        );
    }
}
