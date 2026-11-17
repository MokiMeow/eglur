use super::{body_profile, diagnostic_codes, file, session};
use eglur_diagnostics::DiagnosticSnapshot;
use eglur_driver::{CompilerSession, FileSetRequest, SessionOptions, TrustedCatalog};

#[test]
fn variant_visibility_failure_suppresses_derivative_constructor_errors() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (payload, expression) in [
        ("", "Token::Hidden"),
        ("()", "Token::Hidden()"),
        ("(i64)", "Token::Hidden(7)"),
        ("{ value: i64 }", "Token::Hidden { value: 7 }"),
    ] {
        let source = format!(
            "enum Token {{\n pub Hidden{payload}\n Good\n}}\nfn make() -> Token = {expression}\n"
        );
        let mut session = session(&source);
        session.resolve(&body_profile(), Some(&catalog)).unwrap();
        let snapshot = session.into_diagnostics(None);
        assert_eq!(
            diagnostic_codes(&snapshot),
            ["ENUM_VARIANT_VISIBILITY_FORBIDDEN"],
            "{source}"
        );
    }
}

#[test]
fn enum_level_visibility_keeps_all_unmodified_variants_source_visible() {
    let request = FileSetRequest::new(
        [
            file(
                "dep.eglur",
                concat!(
                    "pub enum Token {\n",
                    " Stop\n Empty()\n Value(i64)\n Named { count: i64 }\n",
                    "}\n",
                ),
            ),
            file(
                "main.eglur",
                concat!(
                    "fn stop() -> dep::Token = dep::Token::Stop\n",
                    "fn empty() -> dep::Token = dep::Token::Empty()\n",
                    "fn value() -> dep::Token = dep::Token::Value(7)\n",
                    "fn named() -> dep::Token = dep::Token::Named { count: 7 }\n",
                ),
            ),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}
