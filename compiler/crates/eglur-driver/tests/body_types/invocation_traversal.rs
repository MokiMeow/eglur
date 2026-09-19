use super::{body_profile, diagnostic_codes, session};
use eglur_driver::{SupportProfile, TrustedCatalog};
use eglur_sema::{CompileCapability, FactState, TypecheckCapability};

fn check_invocations(source: &str, profile: &SupportProfile, expected: usize) {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(source);
    let analysis = session.resolve(profile, Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();

    assert_eq!(resolution.body_types().invocations().len(), expected);
    assert!(!analysis.trace().entries().iter().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
    }));
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn nested_ordinary_calls_typecheck_on_the_default_stack() {
    let depth = 255;
    let source = format!(
        "fn id(value: i64) -> i64 = value\nfn f(x: i64) -> i64 = {}x{}\n",
        "id(".repeat(depth),
        ")".repeat(depth),
    );
    check_invocations(&source, &body_profile(), depth);
}

#[test]
fn nested_generic_calls_typecheck_with_inferred_and_explicit_arguments() {
    let depth = 255;
    for call in ["id(", "id::[i64]("] {
        let source = format!(
            "fn id[T](value: T) -> T = value\nfn f(x: i64) -> i64 = {}x{}\n",
            call.repeat(depth),
            ")".repeat(depth),
        );
        check_invocations(&source, &body_profile(), depth);
    }
}

#[test]
fn mixed_call_and_block_frames_share_the_work_stack() {
    let depth = 120;
    let source = format!(
        "fn id[T](value: T) -> T = value\nfn f(x: i64) -> i64 = {}x{}\n",
        "id({ yield ".repeat(depth),
        " })".repeat(depth),
    );
    check_invocations(&source, &body_profile(), depth);
}

#[test]
fn nested_intrinsic_payloads_typecheck_on_the_default_stack() {
    let depth = 254;
    let source = format!(
        "fn consume[T](value: T) -> Unit = ()\nfn f(x: i64) -> Unit = consume({}x{})\n",
        "Some(".repeat(depth),
        ")".repeat(depth),
    );
    check_invocations(&source, &body_profile(), depth + 1);
}

#[test]
fn chained_ordinary_reduce_by_receivers_stay_outside_tensor_typing() {
    let depth = 255;
    let source = format!(
        "struct Counter {{ fn reduce_by(self: Self) -> Self = self }}\n\
         fn f(counter: Counter) -> Counter = counter{}\n",
        ".reduce_by()".repeat(depth),
    );
    check_invocations(&source, &SupportProfile::semantic_core_v1(), depth);
}

#[test]
fn deep_call_failure_does_not_publish_invocations_or_cascade_errors() {
    let source = format!(
        "fn id(value: i64) -> i64 = value\nfn f() -> i64 = {}true{}\n",
        "id({ yield ".repeat(120),
        " })".repeat(120),
    );
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(&source);
    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();

    assert_eq!(resolution.body_types().invocations().len(), 0);
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["CALL_ARG_TYPE_MISMATCH"]
    );
}

#[test]
fn deep_unsupported_argument_does_not_become_a_known_call_result() {
    let source = format!(
        "fn id(value: i64) -> i64 = value\nfn f() -> i64 = {}[1]{}\n",
        "id(".repeat(254),
        ")".repeat(254),
    );
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(&source);
    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckContainers)
    }));
    let resolution = analysis.resolution().unwrap();
    let f = resolution
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
        resolution.body_types().body(f.id()),
        FactState::Unperformed
    ));
    assert_eq!(resolution.body_types().invocations().len(), 0);
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}
