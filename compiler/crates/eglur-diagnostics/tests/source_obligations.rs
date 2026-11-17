use std::collections::BTreeMap;

use eglur_diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticDataValue, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticRootError, DiagnosticSnapshot, DiagnosticStore, DiagnosticStoreError,
    ErrorLimit, Poison, PrimarySpan, Severity, SourceLabel, SourceObligationKey, SourceSpan,
};
use eglur_source::{ByteOffset, ByteRange, FileId, FileSpan};

fn span(source: &str, start: u32, end: u32) -> SourceSpan {
    SourceSpan::byte_only(
        SourceLabel::new(source).unwrap(),
        FileSpan::new(
            FileId::new(0),
            ByteRange::new(ByteOffset::new(start), ByteOffset::new(end)).unwrap(),
        ),
    )
}

fn diagnostic(name: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::new("TRAIT_IMPL_ASSOC_TYPE_MISMATCH").unwrap(),
        Severity::Error,
        DiagnosticPhase::new("type").unwrap(),
        format!("missing `{name}`"),
        PrimarySpan::Source(span("main.eglur", 10, 20)),
    )
    .with_data(BTreeMap::from([
        ("trait".into(), DiagnosticDataValue::from("Mark")),
        ("associated_type".into(), DiagnosticDataValue::from(name)),
        ("reason".into(), DiagnosticDataValue::from("missing")),
    ]))
}

fn record(store: &mut DiagnosticStore, key: &str, diagnostic: Diagnostic) -> Poison {
    store
        .record_failure(DiagnosticFailure::Root(
            DiagnosticRoot::source_obligation(SourceObligationKey::new(key).unwrap(), diagnostic)
                .unwrap(),
        ))
        .unwrap()
}

#[test]
fn keys_require_nonempty_identity_and_source_roots_keep_admission_guards() {
    assert!(SourceObligationKey::new("").is_none());
    let key = SourceObligationKey::new("required-A").unwrap();
    assert_eq!(key.clone(), key);

    for primary in [
        PrimarySpan::Unavailable,
        diagnostic("A").primary_span().clone(),
    ] {
        for severity in [Severity::Warning, Severity::Note] {
            let invalid = Diagnostic::new(
                DiagnosticCode::new("TYPE_NOTE").unwrap(),
                severity,
                DiagnosticPhase::new("type").unwrap(),
                "not a failed obligation",
                primary.clone(),
            );
            assert_eq!(
                DiagnosticRoot::source_obligation(key.clone(), invalid).unwrap_err(),
                DiagnosticRootError::NonErrorSeverity(severity)
            );
        }
    }

    let invalid = Diagnostic::new(
        DiagnosticCode::new("TYPE_ERROR").unwrap(),
        Severity::Error,
        DiagnosticPhase::new("type").unwrap(),
        "unavailable blame",
        PrimarySpan::Unavailable,
    );
    assert_eq!(
        DiagnosticRoot::source_obligation(key, invalid).unwrap_err(),
        DiagnosticRootError::SourcePrimaryRequired
    );
}

#[test]
fn distinct_obligations_share_blame_without_merging_their_records() {
    for count in [2, 8, 32] {
        let mut store = DiagnosticStore::new();
        for index in 0..count {
            record(
                &mut store,
                &format!("required-{index}"),
                diagnostic(&format!("A{index}")),
            );
        }
        let snapshot = store.finalize(None);
        assert_eq!(snapshot.diagnostics().len(), count);
        assert!(
            snapshot
                .diagnostics()
                .iter()
                .all(|found| { found.primary_span() == diagnostic("A").primary_span() })
        );
    }

    let mut store = DiagnosticStore::new();
    for key in ["declaration-1", "declaration-2"] {
        record(&mut store, key, diagnostic("SameName"));
    }
    let snapshot = store.finalize(None);
    assert_eq!(
        snapshot.diagnostics(),
        [diagnostic("SameName"), diagnostic("SameName")]
    );
}

#[test]
fn rediscovery_dedups_and_conflicts_do_not_advance_or_replace_the_root() {
    for changed in [
        diagnostic("B"),
        Diagnostic::new(
            diagnostic("A").code().clone(),
            Severity::Error,
            DiagnosticPhase::new("type").unwrap(),
            "another rendering",
            diagnostic("A").primary_span().clone(),
        )
        .with_data(diagnostic("A").data().clone()),
        diagnostic("A").with_help("different help"),
    ] {
        let mut store = DiagnosticStore::new();
        let first = record(&mut store, "required-A", diagnostic("A"));
        let checkpoint = store.checkpoint();
        let duplicate = record(&mut store, "required-A", diagnostic("A"));
        store.select_preceding_roots(&first, &duplicate).unwrap();
        let conflicting = DiagnosticRoot::source_obligation(
            SourceObligationKey::new("required-A").unwrap(),
            changed,
        )
        .unwrap();
        assert_eq!(
            store
                .record_failure(DiagnosticFailure::Root(conflicting))
                .unwrap_err(),
            DiagnosticStoreError::ConflictingRoot(diagnostic("A").code().clone())
        );
        assert!(
            store
                .roots_since_within(&checkpoint, &span("main.eglur", 0, 30))
                .unwrap()
                .is_none()
        );
        assert_eq!(store.finalize(None).diagnostics(), [diagnostic("A")]);
    }
}

#[test]
fn obligation_keys_are_scoped_by_code_source_and_byte_range() {
    let mut store = DiagnosticStore::new();
    record(&mut store, "same-target", diagnostic("A"));
    for (code, primary) in [
        ("TYPE_ERROR", span("main.eglur", 10, 20)),
        (
            "TRAIT_IMPL_ASSOC_TYPE_MISMATCH",
            span("other.eglur", 10, 20),
        ),
        ("TRAIT_IMPL_ASSOC_TYPE_MISMATCH", span("main.eglur", 20, 30)),
    ] {
        let error = Diagnostic::new(
            DiagnosticCode::new(code).unwrap(),
            Severity::Error,
            DiagnosticPhase::new("type").unwrap(),
            "independent occurrence",
            PrimarySpan::Source(primary),
        );
        record(&mut store, "same-target", error);
    }
    assert_eq!(store.finalize(None).diagnostics().len(), 4);
}

#[test]
fn shared_blame_ordering_and_limits_depend_on_records_not_internal_keys() {
    let mut snapshots = Vec::new();
    for order in [[0, 1, 2, 3], [3, 1, 0, 2]] {
        for reversed_keys in [false, true] {
            let mut store = DiagnosticStore::new();
            for index in order {
                let key = if reversed_keys { 3 - index } else { index };
                record(
                    &mut store,
                    &format!("key-{key}"),
                    diagnostic(&format!("A{index}")),
                );
            }
            snapshots.push(store.finalize(Some(ErrorLimit::new(2).unwrap())));
        }
    }
    for snapshot in &snapshots {
        assert_eq!(snapshot, &snapshots[0]);
        assert_eq!(
            eglur_diagnostics::render_plain(snapshot),
            eglur_diagnostics::render_plain(&snapshots[0])
        );
        let DiagnosticSnapshot::Rejected(rejected) = snapshot else {
            panic!("an error limit cannot turn failure into success");
        };
        assert_eq!(rejected.omitted_error_count(), 2);
        assert_eq!(rejected.diagnostics(), [diagnostic("A0"), diagnostic("A1")]);
    }
}

#[test]
fn checkpoints_capture_and_suppress_every_shared_blame_obligation() {
    let mut store = DiagnosticStore::new();
    let preceding = record(&mut store, "prior", diagnostic("Prior"));
    let checkpoint = store.checkpoint();
    for key in ["A", "B", "C"] {
        record(&mut store, key, diagnostic(key));
    }
    let outside = Diagnostic::new(
        diagnostic("Outside").code().clone(),
        Severity::Error,
        DiagnosticPhase::new("type").unwrap(),
        "outside container",
        PrimarySpan::Source(span("other.eglur", 10, 20)),
    );
    record(&mut store, "outside", outside.clone());
    let later = store
        .roots_since_within(&checkpoint, &span("main.eglur", 0, 30))
        .unwrap()
        .unwrap();
    store.select_preceding_roots(&preceding, &later).unwrap();
    assert_eq!(
        store.finalize(None).diagnostics(),
        [diagnostic("Prior"), outside]
    );
}

#[test]
fn keyed_roots_retain_store_local_provenance_and_checkpoint_refusal() {
    let mut first = DiagnosticStore::new();
    let root = record(&mut first, "same-key", diagnostic("A"));
    let checkpoint = first.checkpoint();
    let mut second = DiagnosticStore::new();
    let other = record(&mut second, "same-key", diagnostic("A"));
    assert_eq!(
        root.merge(&other).unwrap_err(),
        DiagnosticStoreError::ForeignPoison
    );
    assert_eq!(
        second
            .record_failure(DiagnosticFailure::Derivative(root))
            .unwrap_err(),
        DiagnosticStoreError::ForeignPoison
    );
    assert_eq!(
        second
            .roots_since_within(&checkpoint, &span("main.eglur", 0, 30))
            .unwrap_err(),
        DiagnosticStoreError::ForeignCheckpoint
    );
}
