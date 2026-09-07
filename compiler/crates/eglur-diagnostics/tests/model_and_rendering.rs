use std::collections::BTreeMap;

use eglur_diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticDataValue, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticSnapshot, DiagnosticStore, PrimarySpan, RelatedOrder, RelatedSpan,
    Severity, SourceLabel, SourceProjectionError, SourceSpan, render_plain,
};
use eglur_hir::{DesugaringKind, SourceOrigin};
use eglur_source::{ByteOffset, ByteRange, FileId, FileSpan, LineIndex, SourceFile};

fn file_span(file: FileId, start: u32, end: u32) -> FileSpan {
    FileSpan::new(
        file,
        ByteRange::new(ByteOffset::new(start), ByteOffset::new(end)).unwrap(),
    )
}

#[test]
fn source_projection_derives_text_or_falls_back_to_canonical_bytes() {
    let file = FileId::new(7);
    let source = SourceFile::new(file, "aéz\n".as_bytes().to_vec());
    let lines = LineIndex::new(source.validate().unwrap());
    let label = SourceLabel::new("unicode.eglur").unwrap();

    let projected = SourceSpan::projected(label.clone(), file_span(file, 1, 3), &lines).unwrap();
    assert_eq!(projected.text().unwrap().start().column(), 2);
    assert_eq!(projected.text().unwrap().end().column(), 3);

    let split_scalar = SourceSpan::projected(label.clone(), file_span(file, 2, 3), &lines).unwrap();
    assert!(split_scalar.text().is_none());
    assert_eq!(split_scalar.file_span().range().start(), ByteOffset::new(2));

    assert!(matches!(
        SourceSpan::projected(label.clone(), file_span(file, 0, 99), &lines),
        Err(SourceProjectionError::OutOfBounds(_))
    ));
    assert_eq!(
        SourceSpan::projected(label, file_span(FileId::new(8), 0, 1), &lines),
        Err(SourceProjectionError::FileMismatch {
            span_file: FileId::new(8),
            index_file: file,
        })
    );
}

#[test]
fn source_start_projection_depends_only_on_utf8_validity() {
    let valid = SourceFile::new(FileId::new(9), b"\xef\xbb\xbftext".to_vec());
    let projected = SourceSpan::source_start(SourceLabel::new("valid.eglur").unwrap(), &valid);
    let text = projected
        .text()
        .expect("valid UTF-8 has a source coordinate");
    assert_eq!((text.start().line(), text.start().column()), (1, 1));
    assert!(projected.file_span().range().is_empty());

    let invalid = SourceFile::new(FileId::new(10), vec![0xff]);
    let byte_only = SourceSpan::source_start(SourceLabel::new("invalid.eglur").unwrap(), &invalid);
    assert!(byte_only.text().is_none());
    assert!(byte_only.file_span().range().is_empty());
}

#[test]
fn hir_origin_projection_keeps_the_written_surface_span() {
    let file = FileId::new(3);
    let source = SourceFile::new(file, "value.then(f)".as_bytes().to_vec());
    let lines = LineIndex::new(source.validate().unwrap());
    let written = file_span(file, 0, 13);
    let origin = SourceOrigin::Derived {
        source: written,
        kind: DesugaringKind::PostfixThen,
    };

    let span =
        SourceSpan::from_origin(SourceLabel::new("main.eglur").unwrap(), origin, &lines).unwrap();
    assert_eq!(span.file_span(), written);
    assert_eq!(span.text().unwrap().start().column(), 1);
    assert_eq!(span.text().unwrap().end().column(), 14);
}

#[test]
fn related_spans_default_to_source_order_and_can_preserve_code_order() {
    let file = FileId::new(1);
    let first = RelatedSpan::new(
        SourceSpan::byte_only(SourceLabel::new("a.eglur").unwrap(), file_span(file, 1, 2)),
        "first",
    )
    .unwrap();
    let second = RelatedSpan::new(
        SourceSpan::byte_only(SourceLabel::new("b.eglur").unwrap(), file_span(file, 0, 1)),
        "second",
    )
    .unwrap();
    let same_location_later_label = RelatedSpan::new(
        SourceSpan::byte_only(SourceLabel::new("a.eglur").unwrap(), file_span(file, 1, 2)),
        "later label",
    )
    .unwrap();
    let base = || {
        Diagnostic::new(
            DiagnosticCode::new("NAME_NOT_FOUND").unwrap(),
            Severity::Error,
            DiagnosticPhase::new("resolve").unwrap(),
            "missing",
            PrimarySpan::Source(SourceSpan::byte_only(
                SourceLabel::new("main.eglur").unwrap(),
                file_span(file, 9, 10),
            )),
        )
    };

    let source_order = base().with_related(
        [second.clone(), same_location_later_label, first.clone()],
        RelatedOrder::Source,
    );
    let code_order = base().with_related([second, first], RelatedOrder::CodeDefined);

    assert_eq!(source_order.related()[0].label(), "first");
    assert_eq!(source_order.related()[1].label(), "later label");
    assert_eq!(code_order.related()[0].label(), "second");
    assert!(RelatedSpan::new(source_order.related()[0].span().clone(), "").is_none());
}

#[test]
fn plain_renderer_preserves_field_order_and_structured_details() {
    let file = FileId::new(1);
    let source = SourceFile::new(file, "fn f() { missing }\n".as_bytes().to_vec());
    let lines = LineIndex::new(source.validate().unwrap());
    let label = SourceLabel::new("main.eglur").unwrap();
    let primary = SourceSpan::projected(label.clone(), file_span(file, 9, 16), &lines).unwrap();
    let related_span = SourceSpan::projected(label, file_span(file, 0, 2), &lines).unwrap();
    let related = RelatedSpan::new(related_span, "enclosing declaration starts here").unwrap();
    let mut data = BTreeMap::new();
    data.insert("name".into(), DiagnosticDataValue::from("missing"));
    data.insert("visible".into(), DiagnosticDataValue::from(false));
    let diagnostic = Diagnostic::new(
        DiagnosticCode::new("NAME_NOT_FOUND").unwrap(),
        Severity::Error,
        DiagnosticPhase::new("resolve").unwrap(),
        "name `missing` was not found",
        PrimarySpan::Source(primary),
    )
    .with_help("declare `missing` before use")
    .with_data(data)
    .with_related([related], RelatedOrder::Source);
    let mut store = DiagnosticStore::new();
    store
        .record_failure(DiagnosticFailure::Root(
            DiagnosticRoot::source(diagnostic).unwrap(),
        ))
        .unwrap();
    let snapshot = store.finalize(None);

    assert!(matches!(snapshot, DiagnosticSnapshot::Rejected(_)));
    assert_eq!(
        render_plain(&snapshot),
        concat!(
            "error[NAME_NOT_FOUND] name `missing` was not found\n",
            " --> main.eglur:1:10\n",
            " = help: declare `missing` before use\n",
            " = name: missing\n",
            " = visible: false\n",
            " = related: enclosing declaration starts here\n",
            "   --> main.eglur:1:1-1:3",
        )
    );
}

#[test]
fn unavailable_and_byte_only_locations_render_without_fabricated_coordinates() {
    let byte_only = Diagnostic::new(
        DiagnosticCode::new("LEX_INVALID_CHAR").unwrap(),
        Severity::Error,
        DiagnosticPhase::new("lex").unwrap(),
        "invalid byte",
        PrimarySpan::Source(SourceSpan::byte_only(
            SourceLabel::new("raw.eglur").unwrap(),
            file_span(FileId::new(1), 2, 3),
        )),
    );
    let unavailable = Diagnostic::new(
        DiagnosticCode::new("PACKAGE_ARTIFACT_MISSING").unwrap(),
        Severity::Warning,
        DiagnosticPhase::new("resolve").unwrap(),
        "optional artifact is unavailable",
        PrimarySpan::Unavailable,
    );
    let mut store = DiagnosticStore::new();
    store
        .record_failure(DiagnosticFailure::Root(
            DiagnosticRoot::source(byte_only).unwrap(),
        ))
        .unwrap();
    store.record_observation(unavailable).unwrap();
    let rendered = render_plain(&store.finalize(None));

    assert!(rendered.contains(" --> raw.eglur:bytes[2..3)"));
    assert!(
        rendered.contains("warning[PACKAGE_ARTIFACT_MISSING] optional artifact is unavailable")
    );
    assert!(!rendered.contains("PACKAGE_ARTIFACT_MISSING]\n -->"));
}
