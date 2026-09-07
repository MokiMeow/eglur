use eglur_diagnostics::DiagnosticSnapshot;
use eglur_driver::{
    CompilerSession, FileInput, FileSetRequest, Phase, SessionError, SessionOptions,
    SupportProfile, SupportProfileId, SupportSchemaVersion, TrustedCatalog,
};
use eglur_sema::{
    CompileCapability, DemandDisposition, ResolveCapability, SemanticFact, TypecheckCapability,
};

fn text_file(path: &str, text: &str) -> FileInput {
    FileInput::new(path, text.as_bytes().to_vec())
}

fn file_set(files: impl IntoIterator<Item = FileInput>, entry: &str) -> FileSetRequest {
    FileSetRequest::new(files, entry, Some(".")).unwrap()
}

fn resolver_profile(include_external: bool) -> SupportProfile {
    let mut capabilities = vec![
        CompileCapability::Resolve(ResolveCapability::CollectDeclarations),
        CompileCapability::Resolve(ResolveCapability::ResolveFileSetImports),
        CompileCapability::Resolve(ResolveCapability::ResolveLexicalValues),
        CompileCapability::Resolve(ResolveCapability::ResolveModuleValues),
        CompileCapability::Resolve(ResolveCapability::ResolveQualifiedValues),
        CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures),
        CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEnvironment),
        CompileCapability::Typecheck(TypecheckCapability::CheckBodies),
        CompileCapability::Typecheck(TypecheckCapability::ResolveMembers),
    ];
    if include_external {
        capabilities.push(CompileCapability::Resolve(
            ResolveCapability::ResolveTrustedExternalNames,
        ));
    }
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new(if include_external {
            "resolver-with-external-v1"
        } else {
            "resolver-local-v1"
        })
        .unwrap(),
        capabilities,
    )
    .unwrap()
}

fn first_slice_catalog() -> TrustedCatalog {
    TrustedCatalog::first_slice().unwrap()
}

fn diagnostic_codes(snapshot: &DiagnosticSnapshot) -> Vec<&str> {
    snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect()
}

fn semantic_projection(
    session: &mut CompilerSession,
    profile: &SupportProfile,
    catalog: &TrustedCatalog,
) -> (usize, usize, usize, eglur_driver::CapabilityTrace) {
    let analysis = session.resolve(profile, Some(catalog)).unwrap();
    let resolution = analysis.resolution().expect("collection is supported");
    (
        resolution.declarations().declaration_count(),
        resolution.imports().binding_fact_count(),
        resolution.names().fact_count(),
        analysis.trace().clone(),
    )
}

#[test]
fn semantic_session_uses_one_driver_scc_schedule_and_freezes_support_evidence() {
    let request = file_set(
        [
            text_file(
                "a.eglur",
                "import b::right\npub fn left() -> i64 = right()\n",
            ),
            text_file(
                "b.eglur",
                "import a::left\npub fn right() -> i64 = left()\n",
            ),
        ],
        "a.eglur",
    );
    let profile = resolver_profile(true);
    let catalog = first_slice_catalog();
    let mut session = CompilerSession::build(request, SessionOptions::new(true)).unwrap();

    {
        let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
        let resolution = analysis.resolution().expect("collection is supported");
        assert_eq!(resolution.declarations().import_count(), 2);
        assert_eq!(resolution.import_plan().dependencies().len(), 2);
        assert_eq!(resolution.imports().binding_fact_count(), 2);
        assert!(
            resolution
                .names()
                .facts()
                .any(|(_, fact)| matches!(fact, SemanticFact::Known(_)))
        );
        assert_eq!(analysis.manifest().profile(), profile.id().as_str());
        assert!(analysis.trace().entries().iter().any(|entry| {
            entry.capability() == CompileCapability::Resolve(ResolveCapability::ResolveModuleValues)
                && entry.disposition() == DemandDisposition::Satisfied
        }));
        assert!(!analysis.trace().entries().iter().any(|entry| {
            entry.capability()
                == CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures)
                && entry.disposition() == DemandDisposition::Missing
        }));
    }

    assert!(session.resolution_analysis().is_some());
    assert_eq!(
        session
            .observations()
            .iter()
            .map(|observation| observation.phase())
            .collect::<Vec<_>>(),
        [
            Phase::Lexer,
            Phase::ParserEvents,
            Phase::TreeBuild,
            Phase::HirLowering,
            Phase::DeclarationCollection,
            Phase::ImportPlanning,
            Phase::ImportScc,
            Phase::ImportResolution,
            Phase::NameResolution,
            Phase::DeclarationSignatures,
            Phase::TraitEnvironment,
            Phase::BodyTyping,
            Phase::TraitEvidence,
        ]
    );
    let import_scc = session
        .observations()
        .iter()
        .find(|observation| observation.phase() == Phase::ImportScc)
        .unwrap();
    assert_eq!(
        (import_scc.input_count(), import_scc.output_count()),
        (2, 1)
    );
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn phase_observation_does_not_change_semantic_facts_or_capability_trace() {
    fn request() -> FileSetRequest {
        file_set(
            [
                text_file("dep.eglur", "pub fn value() -> i64 = 1\n"),
                text_file(
                    "main.eglur",
                    "import dep::value\nfn main() -> i64 = value()\n",
                ),
            ],
            "main.eglur",
        )
    }

    let profile = resolver_profile(true);
    let catalog = first_slice_catalog();
    let mut observed = CompilerSession::build(request(), SessionOptions::new(true)).unwrap();
    let mut ordinary = CompilerSession::build(request(), SessionOptions::default()).unwrap();

    assert_eq!(
        semantic_projection(&mut observed, &profile, &catalog),
        semantic_projection(&mut ordinary, &profile, &catalog)
    );
    assert!(!observed.observations().is_empty());
    assert!(ordinary.observations().is_empty());
    assert_eq!(
        diagnostic_codes(&observed.into_diagnostics(None)),
        diagnostic_codes(&ordinary.into_diagnostics(None))
    );
}

#[test]
fn unsupported_collection_is_explicit_and_commits_the_selected_profile_once() {
    let request = file_set(
        [
            text_file("first.eglur", "fn first() -> Unit = ()\n"),
            text_file("second.eglur", "fn second() -> Unit = ()\n"),
        ],
        "first.eglur",
    );
    let profile = SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("empty-v1").unwrap(),
        [],
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::new(true)).unwrap();

    let analysis = session.resolve(&profile, None).unwrap();
    assert!(analysis.resolution().is_none());
    assert_eq!(analysis.trace().missing().count(), 1);
    let missing = analysis.trace().missing().next().unwrap();
    assert_eq!(
        missing.capability(),
        CompileCapability::Resolve(ResolveCapability::CollectDeclarations)
    );
    assert_eq!(missing.locations().len(), 2);

    assert!(matches!(
        session.resolve(&profile, None),
        Err(SessionError::ResolutionAlreadyAttempted)
    ));
    assert!(!session.observations().iter().any(|observation| matches!(
        observation.phase(),
        Phase::ImportPlanning
            | Phase::ImportScc
            | Phase::ImportResolution
            | Phase::NameResolution
            | Phase::DeclarationSignatures
            | Phase::TraitEnvironment
            | Phase::BodyTyping
            | Phase::TraitEvidence
    )));
}

#[test]
fn absent_external_environment_records_missing_support_without_a_name_error() {
    let request =
        FileSetRequest::single("main.eglur", b"fn main() -> i64 = missing\n".to_vec()).unwrap();
    let profile = resolver_profile(false);
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, None).unwrap();
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability()
            == CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames)
    }));
    let snapshot = session.into_diagnostics(None);
    assert!(!diagnostic_codes(&snapshot).contains(&"NAME_NOT_FOUND"));
}

#[test]
fn complete_lookup_reports_name_errors_and_redeclaration_precedes_initializer_lookup() {
    let profile = resolver_profile(true);
    let catalog = first_slice_catalog();
    let missing_request =
        FileSetRequest::single("missing.eglur", b"fn main() -> i64 = missing\n".to_vec()).unwrap();
    let duplicate_request = FileSetRequest::single(
        "duplicate.eglur",
        concat!(
            "fn main!() -> Unit {\n",
            "  let value = 1\n",
            "  let value = missing\n",
            "}\n",
        )
        .as_bytes()
        .to_vec(),
    )
    .unwrap();
    let mut missing = CompilerSession::build(missing_request, SessionOptions::default()).unwrap();
    let mut duplicate =
        CompilerSession::build(duplicate_request, SessionOptions::default()).unwrap();

    missing.resolve(&profile, Some(&catalog)).unwrap();
    duplicate.resolve(&profile, Some(&catalog)).unwrap();

    assert_eq!(
        diagnostic_codes(&missing.into_diagnostics(None)),
        ["NAME_NOT_FOUND"]
    );
    assert_eq!(
        diagnostic_codes(&duplicate.into_diagnostics(None)),
        ["LOCAL_BINDING_REDECLARATION"]
    );
}

#[test]
fn ordinary_tensor_contextual_spellings_do_not_activate_tensor_analysis() {
    let request = FileSetRequest::single(
        "main.eglur",
        concat!(
            "fn view(value: i64) -> i64 = value\n",
            "fn rearrange(value: i64) -> i64 = view(value)\n",
            "fn inflate(value: i64) -> i64 = rearrange(value)\n",
            "fn reduce(value: i64) -> i64 = inflate(value)\n",
            "fn contract(value: i64) -> i64 = reduce(value)\n",
            "fn einop(value: i64) -> i64 = contract(value)\n",
            "fn main() -> i64 = einop(1)\n",
        )
        .as_bytes()
        .to_vec(),
    )
    .unwrap();
    let profile = SupportProfile::semantic_core_v1();
    let catalog = first_slice_catalog();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(analysis.resolution().is_some());
    assert!(!analysis.trace().entries().iter().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
    }));

    let sealed = session.seal(None).unwrap();
    assert!(matches!(
        sealed.diagnostics(),
        DiagnosticSnapshot::Accepted(_)
    ));
    assert!(sealed.checked_program().is_some());
}

#[test]
fn ordinary_dtype_bracket_call_is_rejected_without_tensor_analysis() {
    let request = FileSetRequest::single(
        "main.eglur",
        b"fn id[T](value: T) -> T = value\nfn main() -> i64 = id[i64](7, 8)\n".to_vec(),
    )
    .unwrap();
    let profile = SupportProfile::semantic_core_v1();
    let catalog = first_slice_catalog();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(analysis.resolution().is_some());
    assert!(!analysis.trace().entries().iter().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
    }));
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN"]
    );
}

#[test]
fn source_and_local_shadows_lose_dtype_bracket_privilege_without_tensor_analysis() {
    for source in [
        concat!(
            "import std::tensor::ones\n",
            "fn ones(_dim: Nat) -> i64 = 1\n",
            "fn main() -> i64 = ones[f32](1)\n",
        ),
        concat!(
            "import std::tensor::ones\n",
            "fn main() -> i64 {\n",
            "  let ones = 1\n",
            "  yield ones[i64](7)\n",
            "}\n",
        ),
    ] {
        let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
        let profile = SupportProfile::semantic_core_v1();
        let catalog = first_slice_catalog();
        let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

        let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
        assert!(analysis.resolution().is_some());
        assert!(!analysis.trace().entries().iter().any(|entry| {
            entry.capability()
                == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
        }));
        assert_eq!(
            diagnostic_codes(&session.into_diagnostics(None)),
            ["TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN"]
        );
    }
}

#[test]
fn direct_tensor_constructor_dtype_bracket_requires_constructor_checks() {
    let request = FileSetRequest::single(
        "main.eglur",
        concat!(
            "import std::tensor::ones\n",
            "fn main() -> Unit {\n",
            "  let value = ones[f32](2, 3)\n",
            "  yield ()\n",
            "}\n",
        )
        .as_bytes()
        .to_vec(),
    )
    .unwrap();
    let profile = SupportProfile::semantic_core_v1();
    let catalog = first_slice_catalog();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(analysis.resolution().is_some());
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability()
            == CompileCapability::Typecheck(TypecheckCapability::CheckTensorConstructors)
    }));
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn aliased_tensor_constructor_reexport_preserves_dtype_bracket_privilege() {
    let request = file_set(
        [
            text_file("bridge.eglur", "pub import std::tensor::ones as filled\n"),
            text_file(
                "main.eglur",
                concat!(
                    "import bridge::filled\n",
                    "fn main() -> Unit {\n",
                    "  let value = filled[f32](2, 3)\n",
                    "  yield ()\n",
                    "}\n",
                ),
            ),
        ],
        "main.eglur",
    );
    let profile = SupportProfile::semantic_core_v1();
    let catalog = first_slice_catalog();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(analysis.resolution().is_some());
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability()
            == CompileCapability::Typecheck(TypecheckCapability::CheckTensorConstructors)
    }));
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn ordinary_tensor_source_references_activate_tensor_analysis_without_closure_fallback() {
    for source in [
        concat!(
            "import std::tensor::ones\n",
            "fn main() -> Unit {\n",
            "  let constructor = ones\n",
            "  yield ()\n",
            "}\n",
        ),
        concat!(
            "import std::tensor::ones\n",
            "fn main() -> Unit {\n",
            "  let value = ones(2)\n",
            "  yield ()\n",
            "}\n",
        ),
    ] {
        let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
        let profile = SupportProfile::semantic_core_v1();
        let catalog = first_slice_catalog();
        let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

        let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
        assert!(analysis.resolution().is_some());
        assert!(analysis.trace().missing().any(|entry| {
            entry.capability()
                == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
        }));
        assert!(!analysis.trace().missing().any(|entry| {
            entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckClosures)
        }));
        assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
    }
}

#[test]
fn tensor_syntax_demands_downstream_analysis_without_a_tensor_semantic_product() {
    let request = FileSetRequest::single(
        "main.eglur",
        concat!(
            "fn id(value: i64) -> i64 = value\n",
            "fn main() -> i64 {\n",
            "  let schema = reduce{ h w -> h }.reduce_by((w, id))\n",
            "  yield 1\n",
            "}\n",
        )
        .as_bytes()
        .to_vec(),
    )
    .unwrap();
    let profile = resolver_profile(true);
    let catalog = first_slice_catalog();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(analysis.resolution().is_some());
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
    }));
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn ordinary_reduce_by_calls_do_not_activate_tensor_analysis() {
    for source in [
        concat!(
            "struct Counter {\n",
            "  fn reduce_by(self: Self, pair: (i64, i64)) -> i64 = pair.0\n",
            "}\n",
            "fn main() -> i64 {\n",
            "  let h = 4\n",
            "  let f = 5\n",
            "  let counter = Counter {}\n",
            "  yield counter.reduce_by((h, f))\n",
            "}\n",
        ),
        concat!(
            "struct Counter {\n",
            "  fn reduce_by(self: Self) -> i64 = 1\n",
            "}\n",
            "fn main() -> i64 {\n",
            "  let counter = Counter {}\n",
            "  yield counter.reduce_by()\n",
            "}\n",
        ),
    ] {
        let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
        let profile = SupportProfile::semantic_core_v1();
        let catalog = first_slice_catalog();
        let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

        let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
        assert!(analysis.resolution().is_some());
        assert!(!analysis.trace().entries().iter().any(|entry| {
            entry.capability()
                == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
        }));
        assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
    }
}

#[test]
fn ordinary_reduce_by_materializes_candidate_only_name_failures() {
    let request = FileSetRequest::single(
        "main.eglur",
        concat!(
            "struct Counter {\n",
            "  fn reduce_by(self: Self, pair: (i64, i64)) -> i64 = pair.0\n",
            "}\n",
            "fn main() -> i64 {\n",
            "  let f = 5\n",
            "  let counter = Counter {}\n",
            "  yield counter.reduce_by((missing, f))\n",
            "}\n",
        )
        .as_bytes()
        .to_vec(),
    )
    .unwrap();
    let profile = SupportProfile::semantic_core_v1();
    let catalog = first_slice_catalog();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(!analysis.trace().entries().iter().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
    }));
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["NAME_NOT_FOUND"]
    );
}

#[test]
fn canonical_reduce_by_commits_before_candidate_only_value_lookup() {
    let request = FileSetRequest::single(
        "main.eglur",
        concat!(
            "fn id(value: i64) -> i64 = value\n",
            "fn main() -> i64 {\n",
            "  let schema = reduce{ h w -> h }.reduce_by((missing_axis, id))\n",
            "  yield 1\n",
            "}\n",
        )
        .as_bytes()
        .to_vec(),
    )
    .unwrap();
    let profile = SupportProfile::semantic_core_v1();
    let catalog = first_slice_catalog();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
    }));
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn canonical_single_reduce_by_activates_tensor_analysis() {
    let request = FileSetRequest::single(
        "main.eglur",
        concat!(
            "fn id(value: i64) -> i64 = value\n",
            "fn main() -> i64 {\n",
            "  let schema = reduce{ h w -> h }.reduce_by(id)\n",
            "  yield 1\n",
            "}\n",
        )
        .as_bytes()
        .to_vec(),
    )
    .unwrap();
    let profile = SupportProfile::semantic_core_v1();
    let catalog = first_slice_catalog();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
    }));
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn noncanonical_reduce_by_phase_only_syntax_reports_argument_contract() {
    let source = concat!(
        "fn collapse(value: i64) -> i64 = value\n",
        "fn main(counter: i64) -> i64 {\n",
        "  let value = counter.reduce_by(((h w), collapse))\n",
        "  yield 1\n",
        "}\n",
    );
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let profile = SupportProfile::semantic_core_v1();
    let catalog = first_slice_catalog();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(!analysis.trace().entries().iter().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
    }));
    let diagnostics = session.into_diagnostics(None);
    assert_eq!(
        diagnostic_codes(&diagnostics),
        ["REDUCE_BY_ARGUMENTS_INVALID"]
    );
    let diagnostic = &diagnostics.diagnostics()[0];
    assert!(diagnostic.data().is_empty());
    let span = diagnostic
        .primary_span()
        .as_source()
        .expect("argument diagnostic should be source-bound")
        .file_span()
        .range();
    let expected = "(((h w), collapse))";
    let start = source
        .find(expected)
        .expect("fixture contains argument list");
    assert_eq!(span.start().get() as usize, start);
    assert_eq!(span.end().get() as usize, start + expected.len());
}

#[test]
fn canonical_reduce_by_does_not_fall_back_to_an_empty_ordinary_call() {
    let request = FileSetRequest::single(
        "main.eglur",
        concat!(
            "fn main() -> i64 {\n",
            "  let schema = reduce{ h -> h }.reduce_by()\n",
            "  yield 1\n",
            "}\n",
        )
        .as_bytes()
        .to_vec(),
    )
    .unwrap();
    let profile = SupportProfile::semantic_core_v1();
    let catalog = first_slice_catalog();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(!analysis.trace().entries().iter().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
    }));
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["REDUCE_BY_ARGUMENTS_INVALID"]
    );
}

#[test]
fn recovered_invalid_declaration_poison_suppresses_a_derivative_name_error() {
    let request = FileSetRequest::single(
        "main.eglur",
        b"fn main() -> i64 = hidden\nfn () -> i64 = 1\n".to_vec(),
    )
    .unwrap();
    let profile = resolver_profile(true);
    let catalog = first_slice_catalog();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    session.resolve(&profile, Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let codes = diagnostic_codes(&snapshot);
    assert!(codes.iter().any(|code| code.starts_with("PARSE_")));
    assert!(!codes.contains(&"NAME_NOT_FOUND"));
}
