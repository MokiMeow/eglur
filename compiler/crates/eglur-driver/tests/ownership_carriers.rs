use eglur_diagnostics::DiagnosticSnapshot;
use eglur_driver::{
    CompilerSession, FileSetRequest, SessionOptions, SupportProfile, SupportProfileId,
    SupportSchemaVersion, TrustedCatalog,
};
use eglur_sema::{CompileCapability, ResolveCapability, TypecheckCapability};

fn profile() -> SupportProfile {
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("ownership-carriers-v1").unwrap(),
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
            CompileCapability::Typecheck(TypecheckCapability::AnalyzeInhabitance),
            CompileCapability::Typecheck(TypecheckCapability::CheckFailureFlow),
            CompileCapability::Typecheck(
                TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
            ),
        ],
    )
    .unwrap()
}

fn diagnostics(source: &str) -> DiagnosticSnapshot {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    session.resolve(&profile(), Some(&catalog)).unwrap();
    session.into_diagnostics(None)
}

fn diagnostic_codes(snapshot: &DiagnosticSnapshot) -> Vec<&str> {
    snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect()
}

#[test]
fn reserved_carriers_admit_only_the_direct_ownership_success_type() {
    let snapshot = diagnostics(concat!(
        "fn wrap_some(x: Own[i64]) -> Opt[Own[i64]] { return Some(move x) }\n",
        "fn wrap_none() -> Opt[Own[i64]] = None\n",
        "fn wrap_ok(x: Own[i64]) -> Res[Own[i64], Str] { return Ok(move x) }\n",
        "fn wrap_err() -> Res[Own[i64], Str] = Err(\"failed\")\n",
    ));

    assert!(matches!(snapshot, DiagnosticSnapshot::Accepted(_)));
}

#[test]
fn ordinary_generic_instantiation_remains_ownership_free() {
    let snapshot = diagnostics(concat!(
        "fn identity[T](value: T) -> T = value\n",
        "fn bad(x: Own[i64]) -> Own[i64] = identity(move x)\n",
    ));

    assert_eq!(
        diagnostic_codes(&snapshot),
        ["OWNERSHIP_TYPE_POSITION_FORBIDDEN"]
    );
}

#[test]
fn nested_conditional_ownership_carriers_remain_forbidden() {
    let snapshot = diagnostics(concat!(
        "fn bad(x: Own[i64]) -> Opt[Opt[Own[i64]]] {\n",
        "  return Some(Some(move x))\n",
        "}\n",
    ));

    assert_eq!(
        diagnostic_codes(&snapshot),
        ["OWNERSHIP_TYPE_POSITION_FORBIDDEN"]
    );
}

#[test]
fn complete_local_annotations_admit_roots_and_non_owning_borrow_views() {
    let snapshot = diagnostics(concat!(
        "fn consume(direct_source: Own[i64], optional_source: Opt[Own[i64]], ",
        "result_source: Res[Own[i64], Str]) -> Unit {\n",
        "  let direct: Own[i64] = move direct_source\n",
        "  let optional: Opt[Own[i64]] = move optional_source\n",
        "  let result_root: Res[Own[i64], Str] = move result_source\n",
        "  let mut view: &Own[i64] = &direct\n",
        "  view = &direct\n",
        "  let _ = view\n",
        "  drop_own move direct\n",
        "  drop_own move optional\n",
        "  drop_own move result_root\n",
        "}\n",
    ));

    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {snapshot:?}"
    );
}
