use eglur_driver::{
    CompileAttempt, CompileAttemptError, CompileDisposition, CompilerSession, FileSetRequest,
    ProgramMode, SessionOptions, SupportProfile, SupportProfileId, SupportSchemaVersion, ToolLimit,
};
use eglur_sema::{CompileCapability, ResolveCapability, TypecheckCapability};
use serde_json::Value;

fn full_profile() -> SupportProfile {
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("attempt-full-v1").unwrap(),
        [
            CompileCapability::Resolve(ResolveCapability::CollectDeclarations),
            CompileCapability::Resolve(ResolveCapability::ResolveFileSetImports),
            CompileCapability::Resolve(ResolveCapability::ResolveLexicalValues),
            CompileCapability::Resolve(ResolveCapability::ResolveModuleValues),
            CompileCapability::Resolve(ResolveCapability::ResolveQualifiedValues),
            CompileCapability::Resolve(ResolveCapability::ResolvePatternScopes),
            CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames),
            CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures),
            CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEnvironment),
            CompileCapability::Typecheck(TypecheckCapability::CheckBodies),
            CompileCapability::Typecheck(TypecheckCapability::ResolveMembers),
            CompileCapability::Typecheck(TypecheckCapability::ResolveMethods),
            CompileCapability::Typecheck(TypecheckCapability::ResolveOverloads),
            CompileCapability::Typecheck(TypecheckCapability::AnalyzeInhabitance),
            CompileCapability::Typecheck(TypecheckCapability::CheckFailureFlow),
            CompileCapability::Typecheck(
                TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
            ),
            CompileCapability::Typecheck(TypecheckCapability::CheckBranching),
            CompileCapability::Typecheck(TypecheckCapability::CheckOwnershipFlow),
            CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEvidence),
            CompileCapability::Typecheck(TypecheckCapability::ComputeEffectSummaries),
        ],
    )
    .unwrap()
}

fn minimal_profile() -> SupportProfile {
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("attempt-minimal-v1").unwrap(),
        [CompileCapability::Resolve(
            ResolveCapability::CollectDeclarations,
        )],
    )
    .unwrap()
}

fn attempt(source: &str, profile: &SupportProfile, limit: Option<ToolLimit>) -> CompileAttempt {
    let mut session = CompilerSession::build(
        FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap(),
        SessionOptions::new(true),
    )
    .unwrap();
    session
        .resolve_for(ProgramMode::Executable, profile, None)
        .unwrap();
    CompileAttempt::finalize(session.seal(None).unwrap(), limit).unwrap()
}

#[test]
fn conclusive_attempts_are_the_only_compiler_results() {
    let succeeded = attempt("fn main!() -> Unit = ()\n", &full_profile(), None);
    let result = succeeded.compiler_result().unwrap();
    let payload: Value = serde_json::from_str(&result.to_json().unwrap()).unwrap();
    assert_eq!(succeeded.disposition(), CompileDisposition::Succeeded);
    assert!(result.succeeded());
    assert_eq!(result.exit_status(), 0);
    assert_eq!(payload["kind"], "eglur_compile_result");
    assert_eq!(payload["schema_version"], "0.1");
    assert_eq!(payload["outcome"], serde_json::json!({"kind": "succeeded"}));
    assert!(succeeded.development_attempt().is_none());

    let rejected = attempt("fn main() -> Unit = ()\n", &full_profile(), None);
    let result = rejected.compiler_result().unwrap();
    let payload: Value = serde_json::from_str(&result.to_json().unwrap()).unwrap();
    assert_eq!(rejected.disposition(), CompileDisposition::Rejected);
    assert!(!result.succeeded());
    assert_eq!(result.exit_status(), 1);
    assert_eq!(
        payload["outcome"]["primary_diagnostic"]["code"],
        "PROGRAM_ENTRY_MISSING"
    );
    assert_eq!(
        payload["outcome"]["primary_diagnostic"]["severity"],
        "error"
    );
}

#[test]
fn missing_capability_precedes_and_preserves_source_errors() {
    let unsupported = attempt("fn main!() -> Unit =\n", &minimal_profile(), None);
    assert_eq!(unsupported.disposition(), CompileDisposition::Unsupported);
    assert!(matches!(
        unsupported.compiler_result(),
        Err(CompileAttemptError::Inconclusive(
            CompileDisposition::Unsupported
        ))
    ));
    assert!(unsupported.render_human().contains("error["));
    assert!(unsupported.render_human().contains("tool unsupported:"));

    let payload: Value =
        serde_json::from_str(&unsupported.development_attempt().unwrap().to_json()).unwrap();
    assert_eq!(payload["kind"], "eglur_compile_attempt");
    assert_eq!(payload["disposition"], "unsupported");
    assert!(!payload["diagnostics"].as_array().unwrap().is_empty());
    assert!(
        payload["capability_trace"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["disposition"] == "missing")
    );
}

#[test]
fn tool_limit_precedes_rejection_and_missing_capability() {
    for (source, profile) in [
        ("fn main() -> Unit = ()\n", full_profile()),
        ("fn main!() -> Unit = ()\n", minimal_profile()),
    ] {
        let limited = attempt(source, &profile, ToolLimit::new("driver.wall_clock_budget"));
        assert_eq!(limited.disposition(), CompileDisposition::ToolLimitExceeded);
        assert!(limited.compiler_result().is_err());
        let payload: Value =
            serde_json::from_str(&limited.development_attempt().unwrap().to_json()).unwrap();
        assert_eq!(payload["disposition"], "tool_limit_exceeded");
        assert_eq!(payload["tool_limit"]["key"], "driver.wall_clock_budget");
    }
}

#[test]
fn development_projection_retains_profile_and_phase_observations() {
    let unsupported = attempt("fn main!() -> Unit = ()\n", &minimal_profile(), None);
    let payload: Value =
        serde_json::from_str(&unsupported.development_attempt().unwrap().to_json()).unwrap();

    assert_eq!(payload["mode"], "executable");
    assert_eq!(payload["support"]["profile"], "attempt-minimal-v1");
    assert!(!payload["observations"].as_array().unwrap().is_empty());
    for invalid in [
        "",
        ".budget",
        "driver..budget",
        "Driver.budget",
        "driver budget",
    ] {
        assert!(ToolLimit::new(invalid).is_none(), "accepted {invalid:?}");
    }
}

#[test]
fn direct_and_postfix_callee_holes_remain_explicitly_unsupported() {
    for expression in [
        "_(1)",
        "1.then(_)",
        "_(1, 2)",
        "(1, 2).apply(_)",
        "_ { || 1 }",
        "(_)(1)",
        "((_))(1)",
        "1.then((_))",
        "((1, 2)).apply((_))",
        "(_) { || 1 }",
    ] {
        let source = format!("fn main!() -> Unit {{\nlet partial = {expression}\nyield ()\n}}\n");
        let mut session = CompilerSession::build(
            FileSetRequest::single("main.eglur", source.into_bytes()).unwrap(),
            SessionOptions::new(true),
        )
        .unwrap();
        session
            .resolve_for(ProgramMode::Executable, &full_profile(), None)
            .unwrap();
        let sealed = session
            .seal(None)
            .unwrap_or_else(|error| panic!("{expression}: {error:?}"));
        assert!(sealed.checked_program().is_none(), "{expression}");

        let unsupported = CompileAttempt::finalize(sealed, None).unwrap();
        assert_eq!(
            unsupported.disposition(),
            CompileDisposition::Unsupported,
            "{expression}"
        );
        assert!(matches!(
            unsupported.compiler_result(),
            Err(CompileAttemptError::Inconclusive(
                CompileDisposition::Unsupported
            ))
        ));

        let payload: Value =
            serde_json::from_str(&unsupported.development_attempt().unwrap().to_json()).unwrap();
        assert!(
            payload["diagnostics"].as_array().unwrap().is_empty(),
            "{payload}"
        );
        assert!(
            payload["capability_trace"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| {
                    entry["capability"] == "typecheck.closures" && entry["disposition"] == "missing"
                }),
            "{payload}"
        );
    }
}
