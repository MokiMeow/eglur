use eglur_driver::{
    CompilerSession, FileSetRequest, SessionOptions, SupportProfile, TrustedCatalog,
};

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
