use std::collections::BTreeMap;

use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticDataValue, DiagnosticFailure,
    DiagnosticPhase, DiagnosticRoot, DiagnosticRootError, DiagnosticSnapshot, DiagnosticStore,
    DiagnosticStoreError, ErrorLimit, PrimarySpan, Severity, SourceLabel, SourceSpan,
};
use eglur_source::{ByteOffset, ByteRange, FileId, FileSpan};

fn source_span(source: &str, file: u32, start: u32, end: u32) -> SourceSpan {
    let range = ByteRange::new(ByteOffset::new(start), ByteOffset::new(end)).unwrap();
    SourceSpan::byte_only(
        SourceLabel::new(source).unwrap(),
        FileSpan::new(FileId::new(file), range),
    )
}

fn diagnostic(
    code: &str,
    severity: Severity,
    source: &str,
    file: u32,
    start: u32,
    message: &str,
) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::new(code).unwrap(),
        severity,
        DiagnosticPhase::new("type").unwrap(),
        message,
        PrimarySpan::Source(source_span(source, file, start, start + 1)),
    )
}

fn source_root(diagnostic: Diagnostic) -> DiagnosticRoot {
    DiagnosticRoot::source(diagnostic).unwrap()
}

#[test]
fn duplicate_discovery_and_derivative_checks_retain_one_root() {
    let mut store = DiagnosticStore::new();
    let error = diagnostic(
        "NAME_NOT_FOUND",
        Severity::Error,
        "main.eglur",
        1,
        8,
        "name `missing` was not found",
    );

    let poison = store
        .record_failure(DiagnosticFailure::Root(source_root(error.clone())))
        .unwrap();
    store
        .record_failure(DiagnosticFailure::Root(source_root(error)))
        .unwrap();
    store
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    store
        .record_failure(DiagnosticFailure::Derivative(poison))
        .unwrap();

    let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(None) else {
        panic!("recorded root must reject compilation");
    };
    assert_eq!(snapshot.diagnostics().len(), 1);
    assert_eq!(snapshot.first_error().code().as_str(), "NAME_NOT_FOUND");
    assert_eq!(snapshot.omitted_error_count(), 0);
}

#[test]
fn conflicting_records_for_one_root_are_rejected() {
    let mut store = DiagnosticStore::new();
    let first = diagnostic(
        "NAME_NOT_FOUND",
        Severity::Error,
        "main.eglur",
        1,
        8,
        "first rendering",
    );
    let second = diagnostic(
        "NAME_NOT_FOUND",
        Severity::Error,
        "main.eglur",
        1,
        8,
        "different rendering",
    );

    store
        .record_failure(DiagnosticFailure::Root(source_root(first)))
        .unwrap();
    assert_eq!(
        store
            .record_failure(DiagnosticFailure::Root(source_root(second)))
            .unwrap_err(),
        DiagnosticStoreError::ConflictingRoot(DiagnosticCode::new("NAME_NOT_FOUND").unwrap())
    );
}

#[test]
fn poison_merge_preserves_roots_and_rejects_cross_store_provenance() {
    let mut store = DiagnosticStore::new();
    let left = store
        .record_failure(DiagnosticFailure::Root(source_root(diagnostic(
            "NAME_NOT_FOUND",
            Severity::Error,
            "a.eglur",
            1,
            1,
            "missing a",
        ))))
        .unwrap();
    let right = store
        .record_failure(DiagnosticFailure::Root(source_root(diagnostic(
            "TYPE_MISMATCH",
            Severity::Error,
            "b.eglur",
            2,
            1,
            "type mismatch",
        ))))
        .unwrap();
    let merged = left.merge(&right).unwrap();
    assert!(format!("{merged:?}").contains("root_count: 2"));
    store
        .record_failure(DiagnosticFailure::Derivative(merged))
        .unwrap();

    let mut other = DiagnosticStore::new();
    let foreign = other
        .record_failure(DiagnosticFailure::Root(source_root(diagnostic(
            "NAME_NOT_FOUND",
            Severity::Error,
            "other.eglur",
            3,
            1,
            "missing",
        ))))
        .unwrap();
    assert_eq!(
        left.merge(&foreign).unwrap_err(),
        DiagnosticStoreError::ForeignPoison
    );
    assert_eq!(
        store
            .record_failure(DiagnosticFailure::Derivative(foreign))
            .unwrap_err(),
        DiagnosticStoreError::ForeignPoison
    );
}

#[test]
fn stage_selection_preserves_prior_and_outside_roots() {
    let mut store = DiagnosticStore::new();
    store
        .record_failure(DiagnosticFailure::Root(source_root(diagnostic(
            "PARSE_ERROR",
            Severity::Error,
            "main.eglur",
            1,
            4,
            "earlier syntax error",
        ))))
        .unwrap();
    let checkpoint = store.checkpoint();
    store
        .record_failure(DiagnosticFailure::Root(source_root(diagnostic(
            "NAME_NOT_FOUND",
            Severity::Error,
            "other.eglur",
            2,
            4,
            "independent alias error",
        ))))
        .unwrap();
    store
        .record_failure(DiagnosticFailure::Root(source_root(diagnostic(
            "VISIBILITY_VIOLATION",
            Severity::Error,
            "main.eglur",
            1,
            10,
            "later impl error",
        ))))
        .unwrap();
    let local = store
        .roots_since_within(&checkpoint, &source_span("main.eglur", 1, 0, 20))
        .unwrap()
        .expect("the impl has one new local root");
    let preceding = store
        .record_failure(DiagnosticFailure::Root(source_root(diagnostic(
            "TRAIT_BOUND_NOT_SATISFIED",
            Severity::Error,
            "main.eglur",
            1,
            6,
            "earlier-stage bound failure",
        ))))
        .unwrap();
    store.select_preceding_roots(&preceding, &local).unwrap();
    store
        .record_failure(DiagnosticFailure::Derivative(local))
        .unwrap();
    assert!(store.has_errors());

    let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(None) else {
        panic!("independent roots must remain retained")
    };
    let codes = snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        codes,
        ["PARSE_ERROR", "TRAIT_BOUND_NOT_SATISFIED", "NAME_NOT_FOUND"]
    );
}

#[test]
fn checkpoints_and_preceding_roots_belong_to_one_store() {
    let mut first = DiagnosticStore::new();
    let checkpoint = first.checkpoint();
    let first_root = first
        .record_failure(DiagnosticFailure::Root(source_root(diagnostic(
            "NAME_NOT_FOUND",
            Severity::Error,
            "a.eglur",
            1,
            1,
            "first",
        ))))
        .unwrap();
    let mut second = DiagnosticStore::new();
    let second_root = second
        .record_failure(DiagnosticFailure::Root(source_root(diagnostic(
            "NAME_NOT_FOUND",
            Severity::Error,
            "b.eglur",
            2,
            1,
            "second",
        ))))
        .unwrap();
    assert_eq!(
        second
            .roots_since_within(&checkpoint, &source_span("b.eglur", 2, 0, 5))
            .unwrap_err(),
        DiagnosticStoreError::ForeignCheckpoint
    );
    assert_eq!(
        first
            .select_preceding_roots(&second_root, &first_root)
            .unwrap_err(),
        DiagnosticStoreError::ForeignPoison
    );
}

#[test]
fn ordering_and_limit_ignore_discovery_order_without_hiding_rejection() {
    let mut store = DiagnosticStore::new();
    for start in [30, 10, 20] {
        let mut data = BTreeMap::new();
        data.insert(
            "position".into(),
            DiagnosticDataValue::Integer(i64::from(start)),
        );
        let error = diagnostic(
            "TYPE_MISMATCH",
            Severity::Error,
            "main.eglur",
            1,
            start,
            &format!("error at {start}"),
        )
        .with_data(data);
        store
            .record_failure(DiagnosticFailure::Root(source_root(error)))
            .unwrap();
    }
    store
        .record_observation(diagnostic(
            "TYPE_NOTE",
            Severity::Note,
            "main.eglur",
            1,
            0,
            "earlier note",
        ))
        .unwrap();

    let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(Some(ErrorLimit::new(2).unwrap()))
    else {
        panic!("positive limit must retain an error");
    };
    let error_starts: Vec<_> = snapshot
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.severity() == Severity::Error)
        .map(|diagnostic| {
            diagnostic
                .primary_span()
                .as_source()
                .unwrap()
                .file_span()
                .range()
                .start()
                .get()
        })
        .collect();

    assert_eq!(error_starts, [10, 20]);
    assert_eq!(snapshot.omitted_error_count(), 1);
    assert_eq!(snapshot.first_error().message(), "error at 10");
    assert_eq!(snapshot.diagnostics()[0].message(), "earlier note");
    assert!(ErrorLimit::new(0).is_none());
}

#[test]
fn independent_roots_across_sources_use_canonical_source_order() {
    let mut store = DiagnosticStore::new();
    for (source, file) in [("z.eglur", 9), ("a.eglur", 1)] {
        store
            .record_failure(DiagnosticFailure::Root(source_root(diagnostic(
                "NAME_NOT_FOUND",
                Severity::Error,
                source,
                file,
                4,
                source,
            ))))
            .unwrap();
    }

    let snapshot = store.finalize(None);
    let sources: Vec<_> = snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            diagnostic
                .primary_span()
                .as_source()
                .unwrap()
                .source()
                .as_str()
        })
        .collect();
    assert_eq!(sources, ["a.eglur", "z.eglur"]);
}

#[test]
fn complete_ordering_uses_location_then_severity_then_code() {
    let mut store = DiagnosticStore::new();
    for code in ["Z_ERROR", "A_ERROR"] {
        store
            .record_failure(DiagnosticFailure::Root(source_root(diagnostic(
                code,
                Severity::Error,
                "main.eglur",
                1,
                4,
                code,
            ))))
            .unwrap();
    }
    store
        .record_observation(diagnostic(
            "A_WARNING",
            Severity::Warning,
            "main.eglur",
            1,
            4,
            "warning",
        ))
        .unwrap();
    let unavailable = Diagnostic::new(
        DiagnosticCode::new("A_BOUNDARY_ERROR").unwrap(),
        Severity::Error,
        DiagnosticPhase::new("resolve").unwrap(),
        "boundary",
        PrimarySpan::Unavailable,
    );
    store
        .record_failure(DiagnosticFailure::Root(
            DiagnosticRoot::boundary(
                BoundaryRootKey::new("package:boundary").unwrap(),
                unavailable,
            )
            .unwrap(),
        ))
        .unwrap();

    let snapshot = store.finalize(None);
    let codes: Vec<_> = snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect();
    assert_eq!(
        codes,
        ["A_ERROR", "Z_ERROR", "A_WARNING", "A_BOUNDARY_ERROR"]
    );
}

#[test]
fn boundary_roots_require_unavailable_blame_and_explicit_identity() {
    let unavailable = Diagnostic::new(
        DiagnosticCode::new("PACKAGE_ARTIFACT_MISSING").unwrap(),
        Severity::Error,
        DiagnosticPhase::new("resolve").unwrap(),
        "package artifact is missing",
        PrimarySpan::Unavailable,
    );
    let key = BoundaryRootKey::new("package:example@1.0.0").unwrap();
    let mut store = DiagnosticStore::new();
    store
        .record_failure(DiagnosticFailure::Root(
            DiagnosticRoot::boundary(key.clone(), unavailable.clone()).unwrap(),
        ))
        .unwrap();
    store
        .record_failure(DiagnosticFailure::Root(
            DiagnosticRoot::boundary(key, unavailable).unwrap(),
        ))
        .unwrap();
    assert_eq!(store.finalize(None).diagnostics().len(), 1);

    let source_error = diagnostic(
        "NAME_NOT_FOUND",
        Severity::Error,
        "main.eglur",
        1,
        0,
        "missing",
    );
    assert_eq!(
        DiagnosticRoot::boundary(BoundaryRootKey::new("wrong-shape").unwrap(), source_error)
            .unwrap_err(),
        DiagnosticRootError::UnavailablePrimaryRequired
    );
}

#[test]
fn errors_cannot_bypass_root_classification() {
    let error = diagnostic(
        "NAME_NOT_FOUND",
        Severity::Error,
        "main.eglur",
        1,
        0,
        "missing",
    );
    let warning = diagnostic(
        "STYLE_WARNING",
        Severity::Warning,
        "main.eglur",
        1,
        0,
        "warning",
    );
    assert_eq!(
        DiagnosticRoot::source(warning).unwrap_err(),
        DiagnosticRootError::NonErrorSeverity(Severity::Warning)
    );

    let mut store = DiagnosticStore::new();
    assert_eq!(
        store.record_observation(error),
        Err(DiagnosticStoreError::ErrorObservation)
    );
    let DiagnosticSnapshot::Accepted(snapshot) = store.finalize(None) else {
        panic!("rejected snapshot cannot be constructed without a root");
    };
    assert!(snapshot.diagnostics().is_empty());
}
