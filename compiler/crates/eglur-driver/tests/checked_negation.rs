use eglur_driver::{
    CompilerSession, FileSetRequest, ProgramMode, SealedCompilation, SessionOptions,
    SupportProfile, TrustedCatalog,
};
use eglur_sema::{CompileCapability, TypecheckCapability};

fn compile(source: &str) -> SealedCompilation {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    session
        .resolve_for(
            ProgramMode::Library,
            &SupportProfile::semantic_core_v1(),
            Some(&TrustedCatalog::first_slice().unwrap()),
        )
        .unwrap();
    session.seal(None).unwrap()
}

fn rejected(source: &str, expected: &str) {
    let compiled = compile(source);
    let missing = compiled
        .trace()
        .missing()
        .map(|demand| demand.capability().stable_key())
        .collect::<Vec<_>>();
    assert!(missing.is_empty(), "{source}\n{missing:?}");
    let diagnostics = compiled.diagnostics().diagnostics();
    assert_eq!(
        diagnostics
            .iter()
            .map(|d| d.code().as_str())
            .collect::<Vec<_>>(),
        [expected],
        "{source}"
    );
    assert!(compiled.checked_program().is_none());
}

#[test]
fn dynamic_integer_negation_requires_a_lexical_checked_region() {
    for dtype in ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"] {
        for expression in ["-x", "-(x)"] {
            rejected(
                &format!("type Scalar = {dtype}\nfn negate(x: Scalar) -> Scalar = {expression}\n"),
                "NUMERIC_CHECKED_CONTEXT_REQUIRED",
            );
        }
    }
    rejected(
        "fn negate() -> i8 { let x: i8 = 0\nyield -x }\n",
        "NUMERIC_CHECKED_CONTEXT_REQUIRED",
    );
}

#[test]
fn checked_in_a_caller_does_not_authorize_a_called_body() {
    rejected(
        concat!(
            "fn negate(x: i8) -> i8 = -x\n",
            "fn caller(x: i8) -> Res[i8, NumericError] = checked { yield negate(x) }\n",
        ),
        "NUMERIC_CHECKED_CONTEXT_REQUIRED",
    );
}

#[test]
fn operand_errors_precede_the_checked_context_requirement() {
    rejected(
        "fn read(x: i8) -> i8 = x\nfn negate() -> i8 = -read(true)\n",
        "CALL_ARG_TYPE_MISMATCH",
    );
}

#[test]
fn const_operands_are_not_misreported_as_missing_checked_context() {
    for source in [
        "fn negate() -> i8 = -(1)\n",
        "fn negate() -> i8 = -(-128)\n",
        "const ONE: i8 = 1\nfn negate() -> i8 = -ONE\n",
        "fn negate() -> Res[i8, NumericError] = checked { yield -(1) }\n",
    ] {
        let compiled = compile(source);
        assert!(compiled.diagnostics().diagnostics().is_empty(), "{source}");
        assert!(compiled.checked_program().is_none());
        assert!(
            compiled.trace().missing().any(|demand| {
                demand.capability()
                    == CompileCapability::Typecheck(
                        TypecheckCapability::EvaluateScalarConstExpressions,
                    )
            }),
            "{source}"
        );
    }
}

#[test]
fn nested_result_does_not_remove_the_checked_boundary() {
    let compiled = compile(concat!(
        "fn negate(x: i8) -> Res[Res[i8, Str], NumericError] = checked {\n",
        "  yield result { yield Ok(-x) }\n",
        "}\n",
    ));
    assert!(compiled.diagnostics().diagnostics().is_empty());
    assert!(
        compiled.checked_program().is_some(),
        "{:?}",
        compiled.trace()
    );
}
