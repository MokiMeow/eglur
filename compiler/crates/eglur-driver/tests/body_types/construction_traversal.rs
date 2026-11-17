use super::{diagnostic_codes, file, session};
use eglur_driver::{
    CompilerSession, FileSetRequest, SessionOptions, SupportProfile, TrustedCatalog,
};
use eglur_sema::{CompileCapability, FactState, TypecheckCapability};

fn check_construction(mut session: CompilerSession) {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let missing = analysis.trace().missing().collect::<Vec<_>>();
    assert!(missing.is_empty(), "{missing:?}");
    assert!(analysis.fingerprints().is_some());

    let diagnostics = session.into_diagnostics(None);
    assert!(
        diagnostic_codes(&diagnostics).is_empty(),
        "{:?}",
        diagnostics.diagnostics()
    );
}

#[test]
fn nested_struct_literals_seal_on_the_default_stack() {
    let depth = 240;
    let source = format!(
        "struct Wrap[T] {{ field value: T }}\nfn f(x: i64) -> Unit {{ let value = {}x{}\nyield () }}\n",
        "Wrap { value: ".repeat(depth),
        " }".repeat(depth),
    );
    check_construction(session(&source));
}

#[test]
fn nested_qualified_struct_literals_seal_on_the_default_stack() {
    let depth = 240;
    let source = format!(
        "fn f(x: i64) -> Unit {{ let value = {}x{}\nyield () }}\n",
        "bridge::Wrap { value: ".repeat(depth),
        " }".repeat(depth),
    );
    let request = FileSetRequest::new(
        [
            file("dep.eglur", "pub struct Wrap[T] { pub field value: T }\n"),
            file("bridge.eglur", "pub import dep::Wrap\n"),
            file("main.eglur", &source),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    check_construction(CompilerSession::build(request, SessionOptions::default()).unwrap());
}

#[test]
fn nested_positional_variants_seal_on_the_default_stack() {
    let depth = 240;
    let source = format!(
        "enum Wrapped[T] {{ Value(T) }}\nfn f(x: i64) -> Unit {{ let value = {}x{}\nyield () }}\n",
        "Wrapped::Value(".repeat(depth),
        ")".repeat(depth),
    );
    check_construction(session(&source));
}

#[test]
fn nested_named_variants_seal_on_the_default_stack() {
    let depth = 240;
    let source = format!(
        "enum Wrapped[T] {{ Value {{ item: T }} }}\nfn f(x: i64) -> Unit {{ let value = {}x{}\nyield () }}\n",
        "Wrapped::Value { item: ".repeat(depth),
        " }".repeat(depth),
    );
    check_construction(session(&source));
}

#[test]
fn mixed_constructor_call_and_member_frames_share_the_work_stack() {
    let depth = 100;
    let source = format!(
        "struct Wrap[T] {{ field value: T }}\nfn id[T](value: T) -> T = value\nfn f(x: i64) -> i64 = {}x{}\n",
        "id(Wrap { value: ".repeat(depth),
        " }).value".repeat(depth),
    );
    check_construction(session(&source));
}

#[test]
fn deep_constructor_failure_preserves_one_diagnostic() {
    let depth = 240;
    let source = format!(
        "struct Wrap[T] {{ field value: T }}\nfn f(x: i64) -> Unit {{ let value = {}x.0{}\nyield () }}\n",
        "Wrap { value: ".repeat(depth),
        " }".repeat(depth),
    );
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(&source);
    session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["TUPLE_PROJECTION_NON_TUPLE"]
    );
}

#[test]
fn deep_unsupported_constructor_payload_stays_unperformed() {
    let depth = 240;
    let source = format!(
        "enum Wrapped[T] {{ Value(T) }}\nfn f() -> Unit {{ let value = {}[1]{}\nyield () }}\n",
        "Wrapped::Value(".repeat(depth),
        ")".repeat(depth),
    );
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(&source);
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckContainers)
    }));
    let resolution = analysis.resolution().unwrap();
    let function = resolution
        .declarations()
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| {
            declaration
                .name()
                .is_some_and(|name| name.spelling() == "f")
        })
        .unwrap();
    assert!(matches!(
        resolution.body_types().body(function.id()),
        FactState::Unperformed
    ));
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn unit_constructor_names_keep_contextual_and_deferred_inference() {
    check_construction(session(concat!(
        "fn take[T](left: Opt[T], right: T) -> Opt[T] = left\n",
        "fn contextual() -> Opt[i64] = None\n",
        "fn qualified() -> Opt[i64] = std::opt::None\n",
        "fn deferred(value: i64) -> Opt[i64] = take(None, value)\n",
    )));
}
