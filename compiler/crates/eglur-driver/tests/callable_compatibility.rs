use eglur_diagnostics::DiagnosticDataValue;
use eglur_driver::{
    CompilerSession, FileSetRequest, SessionOptions, SupportProfile, TrustedCatalog,
};
use eglur_sema::{
    CanonicalTarget, CheckedInvocation, CompileCapability, FactState, PhaseProduct,
    TypecheckCapability,
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

fn supported_diagnostic_codes(source: &str) -> Vec<String> {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert!(!analysis.trace().has_missing(), "{source}");
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

#[test]
fn local_callable_value_call_has_indirect_checked_identity() {
    let source = concat!(
        "fn add_one(value: i64) -> i64 = value\n",
        "fn forward(value: i64) -> i64 {\n",
        "  let f: Fn(i64) -> i64 = add_one\n",
        "  yield f(value)\n",
        "}\n",
    );
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().unwrap();
    let PhaseProduct::Performed(evidence) = resolution.trait_evidence() else {
        panic!("trait evidence must be performed");
    };
    assert!(
        evidence.invocations().any(|(_, fact)| {
            matches!(fact, FactState::Known(CheckedInvocation::Indirect { .. }))
        })
    );
    assert!(analysis.fingerprints().is_some());
    assert!(session.into_diagnostics(None).diagnostics().is_empty());
}

#[test]
fn indirect_callable_calls_use_the_exposed_fn_argument_contract() {
    assert_eq!(
        diagnostic_codes("fn use(f: Fn(Str) -> Str) -> Str = f(7)\n"),
        ["CALL_ARG_TYPE_MISMATCH"]
    );
    assert_eq!(
        diagnostic_codes("fn use(f: Fn(Str) -> Str) -> Str = f()\n"),
        ["CALL_MISSING_ARG"]
    );
    assert_eq!(
        diagnostic_codes("fn use(f: Fn(Str) -> Str) -> Str = f(\"a\", \"b\")\n"),
        ["CALL_TOO_MANY_ARGS"]
    );
}

#[test]
fn fn_value_calls_do_not_expose_declaration_parameter_names() {
    let declaration = "fn identity(value: i64) -> i64 = value\n";

    assert!(
        diagnostic_codes(&format!(
            "{declaration}fn use() -> i64 = identity(value=1)\n"
        ))
        .is_empty()
    );

    for body in [
        "fn use() -> i64 { let f: Fn(i64) -> i64 = identity\nyield f(value=1) }\n",
        "fn use() -> i64 { let f = identity\nyield f(value=1) }\n",
    ] {
        assert_eq!(
            diagnostic_codes(&format!("{declaration}{body}")),
            ["CALL_UNKNOWN_NAMED_ARG"]
        );
    }

    assert_eq!(
        diagnostic_codes("fn use(f: Fn(i64) -> i64) -> i64 = f(value=1)\n"),
        ["CALL_UNKNOWN_NAMED_ARG"]
    );
}

#[test]
fn named_callable_value_incompatible_with_expected_fn_is_rejected() {
    let source = concat!(
        "fn identity(value: i64) -> i64 = value\n",
        "fn accept(f: Fn(Str) -> Str) -> Unit = ()\n",
        "fn use() -> Unit = accept(identity)\n",
    );
    assert_eq!(diagnostic_codes(source), ["CALL_ARG_TYPE_MISMATCH"]);
}

#[test]
fn expected_fn_selects_one_overload() {
    let source = concat!(
        "overload fn identity(value: i64) -> i64 = value\n",
        "overload fn identity(value: Str) -> Str = value\n",
        "fn use() -> i64 {\n",
        "  let f: Fn(i64) -> i64 = identity\n",
        "  yield f(7)\n",
        "}\n",
    );
    assert!(supported_diagnostic_codes(source).is_empty());
}

#[test]
fn expected_fn_rejects_ambiguous_assignable_overloads() {
    let source = concat!(
        "struct NarrowError {}\n",
        "struct OtherError {}\n",
        "overload fn inspect(value: Res[i64, NarrowError]) -> i64 = 1\n",
        "overload fn inspect(value: Res[i64, Union[NarrowError, OtherError]]) -> i64 = 2\n",
        "fn accept(f: Fn(Res[i64, NarrowError]) -> i64) -> Unit = ()\n",
        "fn use() -> Unit = accept(inspect)\n",
    );
    assert_eq!(supported_diagnostic_codes(source), ["OVERLOAD_AMBIGUOUS"]);
}

#[test]
fn failed_generic_bound_does_not_prune_an_expected_fn_overload() {
    let declarations = concat!(
        "trait Mark {}\n",
        "struct Plain {}\n",
        "struct NarrowError {}\n",
        "struct OtherError {}\n",
        "overload fn inspect[T: Mark](value: Res[T, NarrowError]) -> i64 = 1\n",
        "overload fn inspect(value: Res[Plain, Union[NarrowError, OtherError]]) -> i64 = 2\n",
    );
    let coercion = concat!(
        "fn accept(f: Fn(Res[Plain, NarrowError]) -> i64) -> Unit = ()\n",
        "fn use() -> Unit = accept(inspect)\n",
    );

    for marker_impl in ["", "impl Mark for Plain {}\n"] {
        let source = format!("{declarations}{marker_impl}{coercion}");
        assert_eq!(
            supported_diagnostic_codes(&source),
            ["OVERLOAD_AMBIGUOUS"],
            "{source}"
        );
    }
}

#[test]
fn expected_fn_without_compatible_overload_has_its_own_diagnostic() {
    let source = concat!(
        "overload fn identity(value: i64) -> i64 = value\n",
        "overload fn identity(value: Str) -> Str = value\n",
        "fn accept(f: Fn(Bool) -> Bool) -> Unit = ()\n",
        "fn use() -> Unit = accept(identity)\n",
    );
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert!(!analysis.trace().has_missing());

    let diagnostics = session.into_diagnostics(None);
    let [diagnostic] = diagnostics.diagnostics() else {
        panic!("expected one diagnostic: {:?}", diagnostics.diagnostics());
    };
    assert_eq!(diagnostic.code().as_str(), "CALLABLE_VALUE_NO_MATCH");
    assert_eq!(
        diagnostic.data().get("expected_type"),
        Some(&DiagnosticDataValue::from("Fn(Bool) -> Bool"))
    );
    assert!(diagnostic.data().contains_key("candidates"));
    assert!(!diagnostic.data().contains_key("actual_type"));
}

#[test]
fn expected_fn_instantiates_generic_at_value_construction() {
    let source = concat!(
        "trait Readable {\n  type Elem\n  fn read(self: &Self) -> Self::Elem\n}\n",
        "struct Good {}\n",
        "impl Readable for Good {\n  type Elem = i64\n  fn read(self: &Self) -> Self::Elem = 8\n}\n",
        "fn bounded[T: Readable](value: T) -> T::Elem = value.read()\n",
        "fn use(f: Fn(Good) -> i64) -> i64 = f(Good{})\n",
        "fn run() -> i64 = use(bounded)\n",
    );
    let codes = supported_diagnostic_codes(source);
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn generic_fn_value_without_context_is_underconstrained() {
    assert_eq!(
        supported_diagnostic_codes(
            "fn identity[T](value: T) -> T = value\nfn use() -> Unit { let f = identity\nyield () }\n"
        ),
        ["TYPE_ARG_INFERENCE_FAILED"]
    );
}

#[test]
fn conflicting_generic_fn_value_constraints_fail_inference() {
    let source = concat!(
        "fn identity[T](value: T) -> T = value\n",
        "fn use() -> Unit {\n",
        "  let f: Fn(i64) -> Str = identity\n",
        "  yield ()\n",
        "}\n",
    );
    assert_eq!(
        supported_diagnostic_codes(source),
        ["TYPE_ARG_INFERENCE_FAILED"]
    );
}

#[test]
fn expected_fn_generic_bound_failure_is_reported_at_construction() {
    let source = concat!(
        "trait Mark {}\n",
        "struct Plain {}\n",
        "fn bounded[T: Mark](value: T) -> i64 = 1\n",
        "fn use() -> Unit {\n",
        "  let f: Fn(Plain) -> i64 = bounded\n",
        "  yield ()\n",
        "}\n",
    );
    assert_eq!(
        supported_diagnostic_codes(source),
        ["TRAIT_BOUND_NOT_SATISFIED"]
    );
}

#[test]
fn selected_overloaded_generic_fn_value_reports_its_bound_failure() {
    let source = concat!(
        "trait Mark {}\n",
        "struct Plain {}\n",
        "struct Boxed[T] { field value: T }\n",
        "overload fn bounded[T: Mark](value: Boxed[T]) -> i64 = 1\n",
        "overload fn bounded(value: Str) -> i64 = 2\n",
        "fn accept(f: Fn(Boxed[Plain]) -> i64) -> Unit = ()\n",
        "fn use() -> Unit = accept(bounded)\n",
    );
    assert_eq!(
        supported_diagnostic_codes(source),
        ["TRAIT_BOUND_NOT_SATISFIED"]
    );
}

#[test]
fn rejected_overload_does_not_report_a_projection_failure() {
    let source = concat!(
        "trait Readable { type Elem\nfn read(self: &Self) -> Self::Elem }\n",
        "struct Plain {}\n",
        "struct Boxed[T] { field value: T }\n",
        "overload fn choose[T: Readable](value: Boxed[T], label: Str) -> T::Elem = value.value.read()\n",
        "overload fn choose(value: i64, label: i64) -> i64 = 0\n",
        "fn accept(f: Fn(Boxed[Plain], Bool) -> i64) -> Unit = ()\n",
        "fn use() -> Unit = accept(choose)\n",
    );
    assert_eq!(
        supported_diagnostic_codes(source),
        ["CALLABLE_VALUE_NO_MATCH"]
    );
}

#[test]
fn expected_fn_rejects_forbidden_generic_reference_argument() {
    assert_eq!(
        supported_diagnostic_codes(
            "fn read[T](value: T) -> i64 = 1\nfn use() -> Unit { let f: Fn(&i64) -> i64 = read\nyield () }\n"
        ),
        ["REFERENCE_TYPE_POSITION_FORBIDDEN"]
    );
}
