use eglur_diagnostics::{DiagnosticDataValue, DiagnosticSnapshot};
use eglur_driver::{
    CompileAttempt, CompileDisposition, CompilerSession, FileSetRequest, SessionOptions,
    SupportProfile, TrustedCatalog,
};

fn compile(source: &str) -> (DiagnosticSnapshot, CompileDisposition) {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let sealed = session.seal(None).unwrap();
    assert!(!sealed.trace().has_missing(), "{source}");
    let diagnostics = sealed.diagnostics().clone();
    let disposition = CompileAttempt::finalize(sealed, None)
        .unwrap()
        .disposition();
    (diagnostics, disposition)
}

fn assert_failed_bound(source: &str, expected: &str, primary: &str) {
    let (snapshot, disposition) = compile(source);
    assert_eq!(disposition, CompileDisposition::Rejected, "{source}");
    let diagnostics = snapshot.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.code().as_str(), "TRAIT_BOUND_NOT_SATISFIED");
    assert_eq!(
        diagnostic.data().get("trait"),
        Some(&DiagnosticDataValue::from(expected)),
        "{source}"
    );
    let span = diagnostic
        .primary_span()
        .as_source()
        .unwrap()
        .file_span()
        .range();
    assert_eq!(
        source[span.start().as_usize()..span.end().as_usize()].trim(),
        primary,
        "{source}"
    );
}

#[test]
fn callable_failures_follow_written_bounds_not_trait_declaration_order() {
    for traits in [
        "trait Alpha {}\ntrait Zulu {}",
        "trait Zulu {}\ntrait Alpha {}",
    ] {
        for (bounds, expected) in [("Zulu + Alpha", "Zulu"), ("Alpha + Zulu", "Alpha")] {
            for call in ["need(1)", "need::[i64](1)"] {
                let source = format!(
                    "{traits}\nfn need[T: {bounds}](x: T) -> Unit = ()\nfn take() -> Unit = {call}\n"
                );
                assert_failed_bound(&source, expected, call);
            }
        }
    }
}

#[test]
fn inferred_constructor_bounds_share_callable_failure_priority() {
    for (declaration, constructor) in [
        (
            "struct Guarded[T: Zulu + Alpha] { field item: T }",
            "Guarded { item: 1 }",
        ),
        (
            "enum Guarded[T: Zulu + Alpha] { Item(T) }",
            "Guarded::Item(1)",
        ),
        (
            "enum Guarded[T: Zulu + Alpha] { Item { item: T } }",
            "Guarded::Item { item: 1 }",
        ),
    ] {
        let source = format!(
            "trait Alpha {{}}\ntrait Zulu {{}}\n{declaration}\nfn take() -> Unit {{ let value = {constructor}\n return }}\n"
        );
        assert_failed_bound(&source, "Zulu", constructor);
    }
}

#[test]
fn method_and_default_body_instantiations_follow_written_bound_priority() {
    for declaration in [
        "struct Runner { fn run[T: Zulu + Alpha](self: &Self, x: T) -> Unit = () }",
        "trait Runs { fn run[T: Zulu + Alpha](self: &Self, x: T) -> Unit = () }\nstruct Runner {}\nimpl Runs for Runner {}",
    ] {
        let source = format!(
            "trait Alpha {{}}\ntrait Zulu {{}}\n{declaration}\nfn take(r: Runner) -> Unit = r.run(1)\n"
        );
        assert_failed_bound(&source, "Zulu", "r.run(1)");
    }
}

#[test]
fn recursive_impl_bounds_and_type_applications_keep_written_priority() {
    for (body, primary) in [
        (
            "type Need[T: Zulu + Alpha] = Unit\nfn take(x: Need[i64]) -> Unit = x",
            "Need[i64]",
        ),
        (
            "trait Target {}\nstruct Wrapped[T] {}\nimpl[T: Zulu + Alpha] Target for Wrapped[T] {}\nfn need[T: Target](x: T) -> Unit = ()\nfn take(x: Wrapped[i64]) -> Unit = need(x)",
            "need(x)",
        ),
    ] {
        let source = format!("trait Alpha {{}}\ntrait Zulu {{}}\n{body}\n");
        assert_failed_bound(&source, "Zulu", primary);
    }
}

#[test]
fn dependent_bounds_use_the_complete_argument_vector_in_both_directions() {
    for body in [
        "fn pick[A, B: Link[A]](a: A, b: B) -> B = b\nfn take() -> Bool = pick(1, true)",
        "fn pick[A: Link[B], B](a: A, b: B) -> A = a\nfn take() -> Bool = pick(true, 1)",
        "fn pick[A: Link[B], B](a: A, b: B) -> A = a\nfn take() -> Bool = pick::[Bool, i64](true, 1)",
    ] {
        let source = format!("trait Link[A] {{}}\nimpl Link[i64] for Bool {{}}\n{body}\n");
        let (diagnostics, disposition) = compile(&source);
        assert!(
            diagnostics.diagnostics().is_empty(),
            "{source}: {diagnostics:?}"
        );
        assert_eq!(disposition, CompileDisposition::Succeeded, "{source}");
    }

    let source = concat!(
        "trait Link[A] {}\nimpl Link[Str] for Bool {}\n",
        "fn pick[A: Link[B], B](a: A, b: B) -> A = a\n",
        "fn take() -> Bool = pick(true, 1)\n",
    );
    assert_failed_bound(source, "Link[i64]", "pick(true, 1)");
}
