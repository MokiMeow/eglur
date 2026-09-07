use eglur_diagnostics::{DiagnosticDataValue, DiagnosticSnapshot};
use eglur_driver::{
    CompilerSession, FileInput, FileSetRequest, ImportDependency, ImportScheduleError, Phase,
    SessionOptions,
};
use eglur_hir::ItemKind;
use eglur_source::FileId;

fn text_file(path: &str, text: &str) -> FileInput {
    FileInput::new(path, text.as_bytes().to_vec())
}

fn file_set(
    files: impl IntoIterator<Item = FileInput>,
    entry: &str,
    root: Option<&str>,
) -> FileSetRequest {
    FileSetRequest::new(files, entry, root).unwrap()
}

fn valid_module(path: &str) -> FileInput {
    let module = path.strip_suffix(".eglur").unwrap().replace('/', "::");
    text_file(
        path,
        &format!("module {module}\n\npub fn value() -> i64 = 1\n"),
    )
}

fn projection(session: &CompilerSession) -> Vec<(u32, String, String)> {
    session
        .sources()
        .iter()
        .map(|source| {
            (
                source.file().get(),
                source.path().to_owned(),
                source.module_path().as_str().to_owned(),
            )
        })
        .collect()
}

fn diagnostic_codes(snapshot: &DiagnosticSnapshot) -> Vec<&str> {
    snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect()
}

#[test]
fn shuffled_file_sets_preserve_source_identity_and_scc_schedule() {
    let files = [
        valid_module("app/z.eglur"),
        valid_module("app/main.eglur"),
        valid_module("app/a.eglur"),
    ];
    let mut reversed = files.clone();
    reversed.reverse();
    let request = file_set(files, "app/main.eglur", Some("."));
    let reversed_request = file_set(reversed, "app/main.eglur", Some("."));
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let mut reversed_session =
        CompilerSession::build(reversed_request, SessionOptions::default()).unwrap();

    assert_eq!(projection(&session), projection(&reversed_session));
    assert_eq!(session.entry(), FileId::new(1));
    let edges = [
        ImportDependency::new(FileId::new(1), FileId::new(0)),
        ImportDependency::new(FileId::new(0), FileId::new(2)),
        ImportDependency::new(FileId::new(2), FileId::new(0)),
    ];
    let mut reversed_edges = edges;
    reversed_edges.reverse();
    let schedule = session.plan_imports(edges).unwrap();
    let reversed_schedule = reversed_session.plan_imports(reversed_edges).unwrap();
    assert_eq!(schedule, reversed_schedule);
    assert_eq!(
        schedule.units()[0].members(),
        &[FileId::new(0), FileId::new(2)]
    );
    assert_eq!(schedule.units()[1].members(), &[FileId::new(1)]);
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
    assert!(matches!(
        reversed_session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn explicit_source_label_changes_diagnostics_not_mapped_identity() {
    let request = FileSetRequest::new(
        [FileInput::with_source_label(
            "mapped/main.eglur",
            "code",
            b"fn".to_vec(),
        )],
        "mapped/main.eglur",
        None,
    )
    .unwrap();
    let session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let source = &session.sources()[0];
    assert_eq!(source.path(), "mapped/main.eglur");
    assert_eq!(source.label().as_str(), "code");
    assert_eq!(source.module_path().as_str(), "mapped::main");

    let snapshot = session.into_diagnostics(None);
    let primary = snapshot.diagnostics()[0]
        .primary_span()
        .as_source()
        .expect("syntax diagnostic is source-bound");
    assert_eq!(primary.source().as_str(), "code");
}

#[test]
fn reserved_root_precedes_generic_path_failure_and_source_decoding() {
    let request = file_set(
        [
            valid_module("app/main.eglur"),
            text_file("a-bad/item.eglur", "module wrong\n"),
            FileInput::new("std/bad-name.eglur", vec![0xff]),
        ],
        "app/main.eglur",
        Some("."),
    );
    let mut session = CompilerSession::build(request, SessionOptions::new(true)).unwrap();
    assert!(session.build_input_blocked());
    assert!(session.sources().is_empty());
    assert!(session.observations().is_empty());
    assert_eq!(
        session.plan_imports([]),
        Err(ImportScheduleError::FrontendBlocked)
    );

    let snapshot = session.into_diagnostics(None);
    assert_eq!(diagnostic_codes(&snapshot), ["MODULE_ROOT_RESERVED"]);
    let diagnostic = &snapshot.diagnostics()[0];
    let primary = diagnostic.primary_span().as_source().unwrap();
    assert_eq!(primary.source().as_str(), "std/bad-name.eglur");
    assert!(primary.text().is_none());
    assert_eq!(
        diagnostic.data().get("name"),
        Some(&DiagnosticDataValue::from("std"))
    );
}

#[test]
fn generic_path_failure_selects_canonical_source_and_leftmost_component() {
    let request = file_set(
        [
            valid_module("app/main.eglur"),
            text_file("z-bad/item.eglur", "module wrong\n"),
            text_file("a_bad/bad-name/later-bad.eglur", "module wrong\n"),
        ],
        "app/main.eglur",
        Some("."),
    );
    let session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let snapshot = session.into_diagnostics(None);
    assert_eq!(
        diagnostic_codes(&snapshot),
        ["MODULE_PATH_COMPONENT_INVALID"]
    );
    let diagnostic = &snapshot.diagnostics()[0];
    let primary = diagnostic.primary_span().as_source().unwrap();
    assert_eq!(primary.source().as_str(), "a_bad/bad-name/later-bad.eglur");
    let text = primary
        .text()
        .expect("valid UTF-8 path failure has coordinates");
    assert_eq!((text.start().line(), text.start().column()), (1, 1));
    assert_eq!(
        diagnostic.data().get("name"),
        Some(&DiagnosticDataValue::from("bad-name"))
    );
    assert_eq!(
        diagnostic.data().get("index"),
        Some(&DiagnosticDataValue::from(1_i64))
    );
}

#[test]
fn module_header_mismatch_does_not_rename_the_canonical_module() {
    let request = FileSetRequest::single(
        "app/net/http.eglur",
        b"module app::net::ftp\n\nfn main!() -> i64 = 1\n".to_vec(),
    )
    .unwrap();
    let session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    assert_eq!(
        session.sources()[0].module_path().as_str(),
        "app::net::http"
    );
    assert!(session.sources()[0].header_poison().is_some());

    let snapshot = session.into_diagnostics(None);
    assert_eq!(diagnostic_codes(&snapshot), ["MODULE_PATH_MISMATCH"]);
    assert_eq!(
        snapshot.diagnostics()[0]
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range()
            .start()
            .get(),
        0
    );
}

#[test]
fn explicit_root_is_removed_from_canonical_module_paths() {
    let request = file_set(
        [
            text_file("src/main.eglur", "module main\nfn main!() -> i64 = 1\n"),
            text_file(
                "src/net/http.eglur",
                "module net::http\nfn get() -> i64 = 1\n",
            ),
        ],
        "src/main.eglur",
        Some("src"),
    );
    let session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    assert_eq!(
        projection(&session),
        [
            (0, "src/main.eglur".into(), "main".into()),
            (1, "src/net/http.eglur".into(), "net::http".into()),
        ]
    );
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn malformed_module_header_does_not_create_a_derivative_mismatch() {
    let request = FileSetRequest::single(
        "main.eglur",
        b"module wrong::\n\nfn main!() -> i64 = 1\n".to_vec(),
    )
    .unwrap();
    let session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    assert!(session.sources()[0].header_poison().is_none());
    let snapshot = session.into_diagnostics(None);
    assert!(!diagnostic_codes(&snapshot).contains(&"MODULE_PATH_MISMATCH"));
    assert!(
        snapshot
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.phase().as_str() == "parse")
    );
}

#[test]
fn valid_header_mismatch_survives_an_unrelated_parser_error() {
    let request = FileSetRequest::single(
        "main.eglur",
        b"module wrong\n\nfn broken() -> i64 = ,\nfn valid() -> i64 = 1\n".to_vec(),
    )
    .unwrap();
    let session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    assert_eq!(session.sources()[0].module_path().as_str(), "main");
    assert!(session.sources()[0].header_poison().is_some());
    let snapshot = session.into_diagnostics(None);
    let codes = diagnostic_codes(&snapshot);
    assert!(codes.contains(&"MODULE_PATH_MISMATCH"));
    assert!(codes.contains(&"PARSE_EXPECTED_EXPRESSION"));
}

#[test]
fn parser_recovery_retains_valid_declarations_and_exact_poison() {
    let request = FileSetRequest::single(
        "main.eglur",
        concat!(
            "fn first() -> i64 = ,\n",
            "fn middle() -> i64 = 1\n",
            "const broken: i64 = ,\n",
            "fn last() -> i64 = 2\n",
        )
        .as_bytes()
        .to_vec(),
    )
    .unwrap();
    let session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let source = &session.sources()[0];
    let hir = source.hir().unwrap();
    let names = hir
        .roots()
        .iter()
        .filter_map(|&item| match hir.item(item).kind() {
            ItemKind::Function(function) => function.name.text(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(names.contains(&"middle"));
    assert!(names.contains(&"last"));
    assert_eq!(
        source.invalid_facts().len(),
        hir.invalid_occurrences().len()
    );
    assert!(
        source
            .invalid_facts()
            .all(|fact| !fact.occurrence().syntax_causes().is_empty())
    );
    for (cause, _) in hir.syntax_causes() {
        assert!(source.syntax_poison(cause).is_some());
    }

    let snapshot = session.into_diagnostics(None);
    assert_eq!(
        diagnostic_codes(&snapshot),
        ["PARSE_EXPECTED_EXPRESSION", "PARSE_EXPECTED_EXPRESSION"]
    );
}

#[test]
fn invalid_utf8_is_byte_only_and_never_produces_parse_or_hir() {
    let request = FileSetRequest::single("main.eglur", vec![0xff]).unwrap();
    let session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let source = &session.sources()[0];
    assert!(source.parse().is_none());
    assert!(source.hir().is_none());
    assert!(source.source_poison().is_some());

    let snapshot = session.into_diagnostics(None);
    assert_eq!(diagnostic_codes(&snapshot), ["LEX_INVALID_UTF8"]);
    assert!(
        snapshot.diagnostics()[0]
            .primary_span()
            .as_source()
            .unwrap()
            .text()
            .is_none()
    );
}

#[test]
fn opt_in_observations_cover_frontend_hir_and_import_scc_without_semantic_drift() {
    let files = [valid_module("a.eglur"), valid_module("b.eglur")];
    let request = file_set(files.clone(), "a.eglur", Some("."));
    let ordinary_request = file_set(files, "a.eglur", Some("."));
    let mut observed = CompilerSession::build(request, SessionOptions::new(true)).unwrap();
    let ordinary = CompilerSession::build(ordinary_request, SessionOptions::default()).unwrap();
    assert_eq!(projection(&observed), projection(&ordinary));
    for (left, right) in observed.sources().iter().zip(ordinary.sources()) {
        assert_eq!(left.hir(), right.hir());
        assert_eq!(
            left.parse().unwrap().syntax().text(),
            right.parse().unwrap().syntax().text()
        );
    }
    assert!(ordinary.observations().is_empty());

    let schedule = observed
        .plan_imports([
            ImportDependency::new(FileId::new(0), FileId::new(1)),
            ImportDependency::new(FileId::new(1), FileId::new(0)),
        ])
        .unwrap();
    assert_eq!(schedule.units().len(), 1);
    let phases = observed
        .observations()
        .iter()
        .map(|observation| observation.phase())
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        [
            Phase::Lexer,
            Phase::ParserEvents,
            Phase::TreeBuild,
            Phase::HirLowering,
            Phase::ImportScc,
        ]
    );
    let hir = observed
        .observations()
        .iter()
        .find(|observation| observation.phase() == Phase::HirLowering)
        .unwrap();
    assert_eq!((hir.input_count(), hir.output_count()), (2, 2));
    let scc = observed
        .observations()
        .iter()
        .find(|observation| observation.phase() == Phase::ImportScc)
        .unwrap();
    assert_eq!((scc.input_count(), scc.output_count()), (2, 1));
}

#[test]
fn parser_resource_diagnostic_keeps_required_structured_data() {
    let nested = "(".repeat(257);
    let closing = ")".repeat(257);
    let source = format!("fn main() -> i64 = {nested}1{closing}\n");
    let request = FileSetRequest::single("main.eglur", source.into_bytes()).unwrap();
    let session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostic = snapshot
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code().as_str() == "PARSE_RESOURCE_LIMIT_EXCEEDED")
        .expect("nested input should exceed a parser resource limit");
    assert_eq!(diagnostic.data().len(), 3);
    assert!(diagnostic.data().contains_key("resource"));
    assert!(diagnostic.data().contains_key("limit"));
    assert!(diagnostic.data().contains_key("observed_at_least"));
}
