use super::*;
use eglur_driver::{CompileAttempt, ProgramMode, SupportProfile, TrustedCatalog};
use eglur_source::SourceFile;
use eglur_syntax::parse_validated;

fn malformed_declarations(count: usize, tail: &str) -> String {
    let mut text = (0..count)
        .map(|index| format!("fn broken_{index}() -> Unit {tail}\n"))
        .collect::<String>();
    text.push_str("fn main!() -> Res[Str, Str] = Ok(\"good\")\n// trailing input\n");
    text
}

#[test]
fn diagnostic_cap_keeps_every_hir_invalid_tied_to_retained_syntax_roots() {
    for (tail, code) in [
        ("{ let = 0 }", "PARSE_EXPECTED_PATTERN"),
        ("= ,", "PARSE_EXPECTED_EXPRESSION"),
    ] {
        for count in [1, 31, 32, 33, 40] {
            let text = malformed_declarations(count, tail);
            let input = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
            let parse = parse_validated(input.validate().unwrap());
            let expected_hir = eglur_hir::lower(&parse);
            let request = FileSetRequest::single("main.eglur", text.as_bytes().to_vec()).unwrap();
            let session = CompilerSession::build(request, SessionOptions::default()).unwrap();
            let source = &session.sources()[0];
            let hir = source.hir().unwrap();

            assert_eq!(hir, &expected_hir);
            assert_eq!(
                source.invalid_facts().len(),
                hir.invalid_occurrences().len()
            );
            for fact in source.invalid_facts() {
                assert!(!fact.occurrence().syntax_causes().is_empty());
                for &cause in fact.occurrence().syntax_causes() {
                    assert!(source.syntax_poison(cause).is_some());
                }
            }

            let snapshot = session.into_diagnostics(None);
            assert_eq!(
                diagnostic_codes(&snapshot),
                vec![code; parse.diagnostics().len()]
            );
            for (diagnostic, expected) in snapshot.diagnostics().iter().zip(parse.diagnostics()) {
                let eglur_diagnostics::PrimarySpan::Source(span) = diagnostic.primary_span() else {
                    panic!("syntax primary span must retain source identity");
                };
                assert_eq!(span.file_span(), expected.span());
            }
        }
    }
}

#[test]
fn diagnostic_cap_finalizes_as_a_rejected_compile_attempt() {
    let profile = SupportProfile::semantic_core_v1();
    let catalog = TrustedCatalog::first_slice().unwrap();
    for tail in ["{ let = 0 }", "= ,"] {
        for count in [1, 31, 32, 33, 40] {
            let request = FileSetRequest::single(
                "main.eglur",
                malformed_declarations(count, tail).into_bytes(),
            )
            .unwrap();
            let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
            session
                .resolve_for(ProgramMode::Executable, &profile, Some(&catalog))
                .unwrap();
            let attempt = CompileAttempt::finalize(session.seal(None).unwrap(), None).unwrap();

            assert!(matches!(attempt, CompileAttempt::Rejected(_)));
            let projection = attempt.compiler_result().unwrap();
            assert!(!projection.succeeded());
            projection.to_json().unwrap();
        }
    }
}

#[test]
fn diagnostic_cap_normalizes_both_sources_and_keeps_primary_spans_file_local() {
    let first = malformed_declarations(40, "{ let = 0 }");
    let second = format!("module other\n{}", malformed_declarations(40, "= ,"));
    let request = file_set(
        [
            text_file("main.eglur", &first),
            text_file("other.eglur", &second),
        ],
        "main.eglur",
        Some("."),
    );
    let session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let mut expected = Vec::new();
    for source in session.sources() {
        let text = if source.path() == "main.eglur" {
            &first
        } else {
            &second
        };
        let input = SourceFile::new(source.file(), text.as_bytes().to_vec());
        let parse = parse_validated(input.validate().unwrap());
        for fact in source.invalid_facts() {
            assert!(!fact.occurrence().syntax_causes().is_empty());
            for &cause in fact.occurrence().syntax_causes() {
                assert!(source.syntax_poison(cause).is_some());
            }
        }
        expected.extend(
            parse
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.span()),
        );
    }
    let snapshot = session.into_diagnostics(None);
    let actual = snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            let eglur_diagnostics::PrimarySpan::Source(span) = diagnostic.primary_span() else {
                panic!("syntax primary span must retain source identity");
            };
            span.file_span()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}
