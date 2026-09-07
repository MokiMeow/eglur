use eglur_diagnostics::DiagnosticSnapshot;
use eglur_driver::{
    CheckedEntryResult, CompilerSession, FileInput, FileSetRequest, ProgramMode, SessionOptions,
    SupportProfile, SupportProfileId, SupportSchemaVersion, TrustedCatalog,
};
use eglur_sema::{CompileCapability, ResolveCapability, TypecheckCapability};

fn program_profile() -> SupportProfile {
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("checked-program-v1").unwrap(),
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

fn single(source: &str) -> CompilerSession {
    CompilerSession::build(
        FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap(),
        SessionOptions::default(),
    )
    .unwrap()
}

fn file(path: &str, source: &str) -> FileInput {
    FileInput::new(path, source.as_bytes().to_vec())
}

fn first_error_code(snapshot: &DiagnosticSnapshot) -> Option<&str> {
    snapshot
        .first_error()
        .map(|diagnostic| diagnostic.code().as_str())
}

#[test]
fn executable_unit_entry_seals_one_checked_program() {
    let mut session = single("fn main!() -> Unit = ()\n");
    session
        .resolve_for(ProgramMode::Executable, &program_profile(), None)
        .unwrap();

    let sealed = session.seal(None).unwrap();
    let program = sealed.checked_program().expect("valid program seals");
    let entry = program.entry().expect("executable has an entry");

    assert_eq!(sealed.mode(), ProgramMode::Executable);
    assert!(matches!(
        sealed.diagnostics(),
        DiagnosticSnapshot::Accepted(_)
    ));
    assert_eq!(entry.result(), CheckedEntryResult::Unit);
    assert_eq!(program.sources().len(), 1);
}

#[test]
fn non_bang_main_reports_missing_at_source_start() {
    let mut session = single("fn main() -> Unit = ()\n");
    session
        .resolve_for(ProgramMode::Executable, &program_profile(), None)
        .unwrap();

    let sealed = session.seal(None).unwrap();
    let diagnostic = sealed.diagnostics().first_error().unwrap();

    assert!(sealed.checked_program().is_none());
    assert_eq!(diagnostic.code().as_str(), "PROGRAM_ENTRY_MISSING");
    let primary = diagnostic.primary_span().as_source().unwrap();
    assert_eq!(primary.file_span().range().start().get(), 0);
    assert_eq!(primary.file_span().range().end().get(), 0);
    let text = primary.text().unwrap();
    assert_eq!(text.start().line(), 1);
    assert_eq!(text.start().column(), 1);
}

#[test]
fn duplicate_entry_owns_precedence_over_overload_validation() {
    let source = "fn main!() -> Unit = ()\nfn main!() -> Unit = ()\n";
    let mut session = single(source);
    session
        .resolve_for(ProgramMode::Executable, &program_profile(), None)
        .unwrap();

    let sealed = session.seal(None).unwrap();
    let diagnostic = sealed.diagnostics().first_error().unwrap();

    assert_eq!(diagnostic.code().as_str(), "PROGRAM_ENTRY_DUPLICATE");
    assert_eq!(
        diagnostic
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range()
            .start()
            .get(),
        24
    );
    assert_eq!(diagnostic.related().len(), 1);
    assert!(
        !sealed
            .diagnostics()
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code().as_str() == "OVERLOAD_MARKER_REQUIRED")
    );
}

#[test]
fn transparent_result_alias_and_never_are_valid_entry_results() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let cases = [
        (
            "type AppResult[T] = Res[T, Str]\nfn main!() -> AppResult[Str] = Ok(\"ok\")\n",
            false,
        ),
        (
            "import std::test::abort!\nfn main!() -> Never = abort!(\"RUNTIME_ABORT\", \"never\")\n",
            true,
        ),
    ];

    for (source, never) in cases {
        let mut session = single(source);
        session
            .resolve_for(ProgramMode::Executable, &program_profile(), Some(&catalog))
            .unwrap();
        let sealed = session.seal(None).unwrap();
        let result = sealed
            .checked_program()
            .expect("valid normalized entry seals")
            .entry()
            .unwrap()
            .result();

        assert_eq!(matches!(result, CheckedEntryResult::Never), never);
        assert_eq!(matches!(result, CheckedEntryResult::Result { .. }), !never);
    }
}

#[test]
fn invalid_entry_shapes_share_the_normative_signature_diagnostic() {
    for source in [
        "fn main!(value: i64) -> Unit = ()\n",
        "fn main![T]() -> Unit = ()\n",
        "fn main!() -> i64 = 0\n",
    ] {
        let mut session = single(source);
        session
            .resolve_for(ProgramMode::Executable, &program_profile(), None)
            .unwrap();
        let sealed = session.seal(None).unwrap();

        assert_eq!(
            first_error_code(sealed.diagnostics()),
            Some("PROGRAM_ENTRY_SIGNATURE_INVALID"),
            "source: {source}"
        );
        assert!(sealed.checked_program().is_none());
    }
}

#[test]
fn ownership_bearing_result_payload_is_an_invalid_entry() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let source = concat!(
        "fn main!() -> Res[Own[i64], Str] {\n",
        "  let value = 1\n",
        "  yield Ok(own value)\n",
        "}\n",
    );
    let mut session = single(source);
    session
        .resolve_for(ProgramMode::Executable, &program_profile(), Some(&catalog))
        .unwrap();
    let sealed = session.seal(None).unwrap();

    assert_eq!(
        first_error_code(sealed.diagnostics()),
        Some("PROGRAM_ENTRY_SIGNATURE_INVALID")
    );
    assert!(sealed.checked_program().is_none());
}

#[test]
fn selected_entry_source_ignores_main_in_other_modules() {
    let request = FileSetRequest::new(
        [
            file(
                "app/start.eglur",
                "module app::start\nfn main!() -> Unit = ()\n",
            ),
            file(
                "lib/other.eglur",
                "module lib::other\nfn main!() -> Unit = ()\n",
            ),
        ],
        "app/start.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let selected_file = session.entry();
    session
        .resolve_for(ProgramMode::Executable, &program_profile(), None)
        .unwrap();

    let sealed = session.seal(None).unwrap();
    let program = sealed.checked_program().expect("selected entry is valid");

    assert_eq!(
        program.entry().unwrap().declaration().module(),
        program
            .resolution()
            .declarations()
            .module_for_file(selected_file)
            .unwrap()
    );
}

#[test]
fn source_errors_precede_entry_validation() {
    let mut session = single("fn helper() -> i64 {\n  let value =\n  yield value\n}\n");
    session
        .resolve_for(ProgramMode::Executable, &program_profile(), None)
        .unwrap();

    let sealed = session.seal(None).unwrap();

    assert_eq!(
        first_error_code(sealed.diagnostics()),
        Some("PARSE_EXPECTED_EXPRESSION")
    );
    assert!(
        !sealed
            .diagnostics()
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code().as_str() == "PROGRAM_ENTRY_MISSING")
    );
}

#[test]
fn library_and_test_modes_do_not_require_an_entry() {
    for mode in [ProgramMode::Library, ProgramMode::Test] {
        let mut session = single("fn helper() -> Unit = ()\n");
        session.resolve_for(mode, &program_profile(), None).unwrap();
        let sealed = session.seal(None).unwrap();
        let program = sealed.checked_program().expect("non-executable mode seals");

        assert_eq!(program.mode(), mode);
        assert!(program.entry().is_none());
    }
}

#[test]
fn missing_required_analysis_never_fabricates_a_checked_program() {
    let mut session = single("fn main!() -> Unit = ()\n");
    let profile = SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("signature-only-v1").unwrap(),
        [CompileCapability::Resolve(
            ResolveCapability::CollectDeclarations,
        )],
    )
    .unwrap();
    session
        .resolve_for(ProgramMode::Executable, &profile, None)
        .unwrap();

    let sealed = session.seal(None).unwrap();

    assert!(sealed.checked_program().is_none());
    assert!(sealed.trace().has_missing());
}
