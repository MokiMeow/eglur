use eglur_diagnostics::{DiagnosticDataValue, DiagnosticSnapshot};
use eglur_driver::{
    CompileAttempt, CompileDisposition, CompilerSession, FileSetRequest, SessionOptions,
    SupportProfile, TrustedCatalog,
};
use eglur_sema::{DeclarationKind, FactState};

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

fn assert_source_bound_result(source: &str, code: Option<&str>) {
    let (snapshot, disposition) = compile(source);
    let diagnostics = snapshot.diagnostics();
    if let Some(code) = code {
        assert_eq!(disposition, CompileDisposition::Rejected, "{source}");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code().as_str() == code),
            "{source}: {diagnostics:?}"
        );
    } else {
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        assert_eq!(disposition, CompileDisposition::Succeeded, "{source}");
    }
}

#[test]
fn source_projection_uses_the_complete_bound_set_in_every_order() {
    for bounds in [
        "Uses[T::Item]+More[T::Item]+HasItem",
        "More[T::Item]+Uses[T::Item]+HasItem",
        "Uses[T::Item]+HasItem+More[T::Item]",
        "More[T::Item]+HasItem+Uses[T::Item]",
        "HasItem+Uses[T::Item]+More[T::Item]",
        "HasItem+More[T::Item]+Uses[T::Item]",
    ] {
        assert_source_bound_result(
            &format!(
                "trait HasItem {{ type Item }}\ntrait Uses[A] {{}}\ntrait More[A] {{}}\nfn valid[T:{bounds}]() -> Unit = ()\n"
            ),
            None,
        );
    }
}

#[test]
fn projected_declaring_references_support_forward_parameters_and_bound_order() {
    for bounds in ["Has[U::Item]+Uses[T::Item]", "Uses[T::Item]+Has[U::Item]"] {
        for parameters in [
            format!("T:{bounds},U:Other[i64]"),
            format!("U:Other[i64],T:{bounds}"),
        ] {
            assert_source_bound_result(
                &format!(
                    "trait Has[A] {{ type Item }}\ntrait Other[A] {{ type Item }}\ntrait Uses[A] {{}}\nfn valid[{parameters}]() -> Unit = ()\n"
                ),
                None,
            );
        }
    }
}

#[test]
fn complete_declaring_references_distinguish_ambiguity_from_duplicates() {
    for bounds in [
        "Has[i32]+Has[i64]+Uses[T::Item]",
        "Uses[T::Item]+Has[i32]+Has[i64]",
        "Has[i32]+Uses[T::Item]+Other[i64]",
        "Other[i64]+Uses[T::Item]+Has[i32]",
    ] {
        assert_source_bound_result(
            &format!(
                "trait Has[A] {{ type Item }}\ntrait Other[A] {{ type Item }}\ntrait Uses[A] {{}}\nfn bad[T:{bounds}]() -> Unit = ()\n"
            ),
            Some("ASSOCIATED_TYPE_PROJECTION_INVALID"),
        );
    }
    for bounds in [
        "Has[i64]+Has[Alias]+Uses[T::Item]",
        "Uses[T::Item]+Has[Alias]+Has[i64]",
    ] {
        assert_source_bound_result(
            &format!(
                "trait Has[A] {{ type Item }}\ntrait Uses[A] {{}}\ntype Alias = i64\nfn bad[T:{bounds}]() -> Unit = ()\n"
            ),
            Some("TRAIT_BOUND_INVALID"),
        );
    }
}

#[test]
fn bound_reference_cycles_select_the_first_participating_projection() {
    for (prefix, parameters, primary) in [
        ("", "T:Has[T::Item]", "T::Item"),
        ("", "T:Has[U::Item],U:Has[T::Item]", "U::Item"),
        ("", "U:Has[T::Item],T:Has[U::Item]", "T::Item"),
        (
            "struct Wrap[A] { field value: A }\n",
            "T:Has[Wrap[T::Item]]",
            "T::Item",
        ),
        (
            "type Identity[A] = A\n",
            "T:Has[Identity[T::Item]]",
            "T::Item",
        ),
        (
            "type Identity[A] = A\ntype Nested[A] = Identity[A]\n",
            "T:Has[Nested[T::Item]]",
            "T::Item",
        ),
    ] {
        let source =
            format!("trait Has[A] {{ type Item }}\n{prefix}fn bad[{parameters}]() -> Unit = ()\n");
        let (snapshot, disposition) = compile(&source);
        assert_eq!(disposition, CompileDisposition::Rejected, "{source}");
        let diagnostics = snapshot.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(
            diagnostics[0].code().as_str(),
            "GENERIC_BOUND_REFERENCE_CYCLE",
            "{source}"
        );
        let range = diagnostics[0]
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range();
        assert_eq!(
            &source[range.start().as_usize()..range.end().as_usize()],
            primary,
            "{source}"
        );
    }
}

#[test]
fn finite_mutual_bounds_and_completed_projection_sharing_are_not_cycles() {
    for parameters in [
        "T:Has[U],U:Has[T]",
        "T:Has[Wrap[T]]",
        "T:Has[i64]+Uses[(T::Item,T::Item)]",
        "T:Has[U::Item],U:Has[i64]",
    ] {
        assert_source_bound_result(
            &format!(
                "trait Has[A] {{ type Item }}\ntrait Uses[A] {{}}\nstruct Wrap[A] {{ field value: A }}\nfn valid[{parameters}]() -> Unit = ()\n"
            ),
            None,
        );
    }
}

#[test]
fn alias_erasure_removes_only_construction_dependencies_not_source_validation() {
    for aliases in [
        "type Erase[A] = i64",
        "type Drop[A] = i64\ntype Erase[A] = Drop[A]",
    ] {
        let prelude = format!("trait Has[A] {{ type Item }}\n{aliases}\n");
        assert_source_bound_result(
            &format!("{prelude}fn valid[T:Has[Erase[T::Item]]]() -> Unit = ()\n"),
            None,
        );
        for (argument, code) in [
            ("Missing", "NAME_NOT_FOUND"),
            ("&i64", "REFERENCE_TYPE_POSITION_FORBIDDEN"),
            ("T::Missing", "ASSOCIATED_TYPE_PROJECTION_INVALID"),
            ("T::Item::Next", "ASSOCIATED_TYPE_PROJECTION_INVALID"),
            ("Erase[i64,i64]", "TYPE_ARG_ARITY_MISMATCH"),
        ] {
            assert_source_bound_result(
                &format!("{prelude}fn bad[T:Has[Erase[{argument}]]]() -> Unit = ()\n"),
                Some(code),
            );
        }
    }
    assert_source_bound_result(
        "trait Has[A] { type Item }\ntrait Marker {}\ntype Erase[A:Marker] = i64\nfn bad[T:Has[Erase[T::Item]]]() -> Unit = ()\n",
        Some("TRAIT_BOUND_NOT_SATISFIED"),
    );
}

#[test]
fn alias_projection_templates_keep_their_own_declaring_references() {
    assert_source_bound_result(
        "trait Has[A] { type Item }\ntrait Uses[A] {}\ntype Projected[A:Has[i64]] = A::Item\nfn valid[T:Uses[Projected[T]]+Has[i64]]() -> Unit = ()\n",
        None,
    );
    assert_source_bound_result(
        "trait Has[A] { type Item }\ntype First[A] = Second[A]\ntype Second[A] = First[A]\nfn bad[T:Has[First[i64]]]() -> Unit = ()\n",
        Some("TYPE_ALIAS_CYCLE"),
    );
    assert_source_bound_result(
        "trait Has[A] { type Item }\ntrait Uses[A] {}\ntype Projected[A,B:Has[A]] = B::Item\nfn valid[X,Y:Has[X]+Uses[Projected[X,Y]]]() -> Unit = ()\n",
        None,
    );
    assert_source_bound_result(
        "trait Has[A] { type Item }\ntype Second[A,B] = B\nfn valid[T:Has[Second[T::Item,i64]]]() -> Unit = ()\n",
        None,
    );
}

#[test]
fn invalid_complete_bounds_do_not_publish_the_function_and_preserve_later_functions() {
    for declaration in [
        "fn bad[T:Has[T::Item]]() -> Unit = ()",
        "fn bad[T:Other[T::Item]+Has[i64]]() -> Unit = ()",
        "fn bad[T:Uses[U::Item],U:Has[Erase[Missing]]]() -> Unit = ()",
        "fn bad[U:Has[Erase[Missing]],T:Uses[U::Item]]() -> Unit = ()",
    ] {
        let source = format!(
            "trait Has[A] {{ type Item }}\ntrait Other[A] {{ type Item }}\ntrait Uses[A] {{}}\ntype Erase[A] = i64\n{declaration}\nfn after() -> Unit = ()"
        );
        let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
        let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
        let catalog = TrustedCatalog::first_slice().unwrap();
        let analysis = session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert!(!analysis.trace().has_missing(), "{source}");
        let resolution = analysis.resolution().unwrap();
        for declaration in resolution
            .declarations()
            .modules()
            .flat_map(|module| module.declarations())
            .filter(|declaration| declaration.kind() == DeclarationKind::Function)
        {
            let name = declaration.name().unwrap().text();
            let state = resolution.signatures().source_declaration(declaration.id());
            if name == "bad" {
                assert!(
                    matches!(state, FactState::Poisoned(_)),
                    "{source}: {state:?}"
                );
            }
            if name == "after" {
                assert!(matches!(state, FactState::Known(_)), "{source}: {state:?}");
            }
        }
        let sealed = session.seal(None).unwrap();
        assert_eq!(
            CompileAttempt::finalize(sealed, None)
                .unwrap()
                .disposition(),
            CompileDisposition::Rejected
        );
    }
}
