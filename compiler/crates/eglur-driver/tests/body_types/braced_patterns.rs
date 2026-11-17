use super::{diagnostic_codes, file};
use eglur_driver::{
    CompilerSession, FileSetRequest, SessionOptions, SupportProfile, TrustedCatalog,
};
use eglur_sema::{CompileCapability, TypecheckCapability};

fn pattern_session(main: &str) -> CompilerSession {
    CompilerSession::build(
        FileSetRequest::new(
            [
                file(
                    "bridge.eglur",
                    concat!(
                        "pub import dep::Record as Shared\npub import dep::Choice\n",
                        "pub import std::opt::Some as Shared\n",
                        "pub import dep::helper as FunctionAlias\n",
                        "pub import std::opt::Some as ReSome\n",
                        "pub import std::opt::None as ReNone\n",
                        "pub import std::res::Ok as ReOk\n",
                        "pub import std::res::Err as ReErr\n",
                        "import std::opt::Some as PrivateSome\n",
                    ),
                ),
                file(
                    "dep.eglur",
                    concat!(
                        "pub struct Record { pub field value: i64 }\n",
                        "struct Private { field value: i64 }\n",
                        "pub type Alias = Record\n",
                        "pub fn helper() -> Unit = ()\n",
                        "pub enum Choice {\n UnitValue\n TupleValue(i64)\n Named { value: i64 }\n}\n",
                    ),
                ),
                file("main.eglur", main),
            ],
            "main.eglur",
            Some("."),
        )
        .unwrap(),
        SessionOptions::default(),
    )
    .unwrap()
}

#[test]
fn resolved_braced_patterns_retain_the_advanced_pattern_gap() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (ty, head) in [
        ("dep::Record", "Record"),
        ("dep::Record", "Imported"),
        ("dep::Record", "dep::Record"),
        ("dep::Record", "bridge::Shared"),
        ("dep::Record", "Alias"),
        ("dep::Record", "dep::Alias"),
        ("dep::Choice", "dep::Choice::Named"),
        ("dep::Choice", "bridge::Choice::Named"),
    ] {
        let source = format!(
            "import dep::Record\nimport dep::Record as Imported\nimport dep::Alias\nfn f(input: {ty}) -> i64 = match input {{ {head} {{ value }} => value }}\n"
        );
        let mut session = pattern_session(&source);
        let analysis = session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert!(
            analysis.trace().missing().any(|entry| {
                entry.capability()
                    == CompileCapability::Typecheck(TypecheckCapability::CheckAdvancedPatterns)
            }),
            "{head}"
        );

        let sealed = session.seal(None).unwrap();
        assert!(sealed.checked_program().is_none(), "{head}");
        assert!(
            diagnostic_codes(sealed.diagnostics()).is_empty(),
            "{head}: {:?}",
            sealed.diagnostics().diagnostics()
        );
    }
}

#[test]
fn braced_pattern_lookup_preserves_head_and_payload_failures() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (ty, pattern, expected) in [
        ("dep::Record", "dep::Missing { value }", "NAME_NOT_FOUND"),
        (
            "dep::Record",
            "bridge::FunctionAlias { value }",
            "NAME_NOT_FOUND",
        ),
        (
            "dep::Record",
            "dep::Private { value }",
            "VISIBILITY_VIOLATION",
        ),
        (
            "Opt[i64]",
            "bridge::PrivateSome { value }",
            "VISIBILITY_VIOLATION",
        ),
        (
            "dep::Choice",
            "dep::Choice::UnitValue {}",
            "PATTERN_PAYLOAD_FORM_MISMATCH",
        ),
        (
            "dep::Choice",
            "dep::Choice::TupleValue { value }",
            "PATTERN_PAYLOAD_FORM_MISMATCH",
        ),
        (
            "dep::Record",
            "dep::Choice::Named { value }",
            "PATTERN_TYPE_MISMATCH",
        ),
        ("dep::Record", "dep::Record(value)", "NAME_NOT_FOUND"),
    ] {
        let source = format!("fn f(input: {ty}) -> i64 = match input {{ {pattern} => 0 }}\n");
        let mut session = pattern_session(&source);
        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        let diagnostics = session.into_diagnostics(None);
        assert_eq!(diagnostic_codes(&diagnostics), [expected], "{pattern}");
    }
}

#[test]
fn qualified_residual_braced_patterns_keep_payload_form_diagnostics() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (ty, head) in [
        ("Opt[i64]", "std::opt::Some"),
        ("Opt[i64]", "std::opt::None"),
        ("Res[i64, Str]", "std::res::Ok"),
        ("Res[i64, Str]", "std::res::Err"),
        ("Opt[i64]", "bridge::ReSome"),
        ("Opt[i64]", "bridge::ReNone"),
        ("Res[i64, Str]", "bridge::ReOk"),
        ("Res[i64, Str]", "bridge::ReErr"),
    ] {
        let source =
            format!("fn f(input: {ty}) -> i64 = match input {{ {head} {{ value }} => 0 }}\n");
        let mut session = pattern_session(&source);
        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert_eq!(
            diagnostic_codes(&session.into_diagnostics(None)),
            ["PATTERN_PAYLOAD_FORM_MISMATCH"],
            "{head}"
        );
    }
}

#[test]
fn qualified_residual_braced_expressions_keep_payload_form_diagnostics() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (ty, head) in [
        ("Opt[i64]", "std::opt::Some"),
        ("Opt[i64]", "std::opt::None"),
        ("Res[i64, Str]", "std::res::Ok"),
        ("Res[i64, Str]", "std::res::Err"),
        ("Opt[i64]", "bridge::ReSome"),
        ("Opt[i64]", "bridge::ReNone"),
        ("Res[i64, Str]", "bridge::ReOk"),
        ("Res[i64, Str]", "bridge::ReErr"),
    ] {
        let source = format!("fn f() -> {ty} = {head} {{ value: 0 }}\n");
        let mut session = pattern_session(&source);
        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert_eq!(
            diagnostic_codes(&session.into_diagnostics(None)),
            ["CONSTRUCTOR_PAYLOAD_FORM_MISMATCH"],
            "{head}"
        );
    }
}
