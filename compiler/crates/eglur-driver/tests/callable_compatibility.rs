use eglur_driver::{
    CompilerSession, FileSetRequest, SessionOptions, SupportProfile, TrustedCatalog,
};
use eglur_sema::{CanonicalTarget, CompileCapability, TypecheckCapability};

fn diagnostic_codes(source: &str) -> Vec<String> {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    session
        .into_diagnostics(None)
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str().to_owned())
        .collect()
}

#[test]
fn callable_argument_accepts_wider_input_and_narrower_result() {
    let source = concat!(
        "fn accept(f: Fn(&mut i64) -> Res[i64, Union[Str, Bool]]) -> Unit = ()\n",
        "fn forward(f: Fn(&i64) -> Res[i64, Str]) -> Unit = accept(f)\n",
    );
    let codes = diagnostic_codes(source);
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn callable_argument_rejects_narrower_input() {
    let source = concat!(
        "fn accept(f: Fn(&i64) -> i64) -> Unit = ()\n",
        "fn forward(f: Fn(&mut i64) -> i64) -> Unit = accept(f)\n",
    );
    assert_eq!(diagnostic_codes(source), ["CALL_ARG_TYPE_MISMATCH"]);
}

#[test]
fn callable_argument_rejects_wider_result_error() {
    let source = concat!(
        "fn accept(f: Fn() -> Res[i64, Str]) -> Unit = ()\n",
        "fn forward(f: Fn() -> Res[i64, Union[Str, Bool]]) -> Unit = accept(f)\n",
    );
    assert_eq!(diagnostic_codes(source), ["CALL_ARG_TYPE_MISMATCH"]);
}

#[test]
fn callable_argument_rejects_arity_mismatch() {
    let source = concat!(
        "fn accept(f: Fn(i64) -> i64) -> Unit = ()\n",
        "fn forward(f: Fn(i64, i64) -> i64) -> Unit = accept(f)\n",
    );
    assert_eq!(diagnostic_codes(source), ["CALL_ARG_TYPE_MISMATCH"]);
}

#[test]
fn ordinary_named_function_value_is_typed_without_closure_support() {
    let source = concat!(
        "fn add_one(value: i64) -> i64 = value\n",
        "fn forward(value: i64) -> i64 {\n",
        "  let f: Fn(i64) -> i64 = add_one\n",
        "  yield value\n",
        "}\n",
    );
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().unwrap();
    let values = resolution
        .body_types()
        .callable_values()
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 1);
    let selected = resolution
        .body_types()
        .callable_instance(values[0].1)
        .unwrap();
    let signature = resolution
        .signatures()
        .callable(selected.signature())
        .unwrap();
    let CanonicalTarget::Source(target) = signature.declaration() else {
        panic!("source function value selected an external target");
    };
    let declaration = resolution.declarations().declaration(*target).unwrap();
    assert_eq!(declaration.name().unwrap().spelling(), "add_one");
    assert!(analysis.fingerprints().is_some());

    assert!(
        !analysis.trace().missing().any(|entry| {
            entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckClosures)
        }),
        "a named function value must not demand closure support"
    );
    assert!(session.into_diagnostics(None).diagnostics().is_empty());
}

#[test]
fn ordinary_named_function_value_infers_its_local_callable_type() {
    let source = concat!(
        "fn add_one(value: i64) -> i64 = value\n",
        "fn forward(value: i64) -> Unit {\n",
        "  let f = add_one\n",
        "  yield ()\n",
        "}\n",
    );
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();

    assert_eq!(
        analysis
            .resolution()
            .unwrap()
            .body_types()
            .callable_values()
            .len(),
        1
    );
    assert!(!analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckClosures)
    }));
    assert!(session.into_diagnostics(None).diagnostics().is_empty());
}
