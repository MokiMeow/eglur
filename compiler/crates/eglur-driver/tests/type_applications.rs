use eglur_diagnostics::{DiagnosticDataValue, DiagnosticSnapshot};
use eglur_driver::{
    CompileAttempt, CompileDisposition, CompilerSession, FileInput, FileSetRequest, SessionOptions,
    SupportProfile, TrustedCatalog,
};

#[path = "type_applications/poison.rs"]
mod poison;

#[path = "type_applications/callable_poison.rs"]
mod callable_poison;

fn session(source: &str) -> CompilerSession {
    let request = FileSetRequest::new(
        [FileInput::new("main.eglur", source.as_bytes().to_vec())],
        "main.eglur",
        None,
    )
    .unwrap();
    CompilerSession::build(request, SessionOptions::new(false)).unwrap()
}

fn compile(source: &str) -> (Vec<String>, CompileDisposition) {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(source);
    session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let sealed = session.seal(None).unwrap();
    assert!(!sealed.trace().has_missing(), "{source}");
    let codes = match sealed.diagnostics() {
        DiagnosticSnapshot::Accepted(_) => Vec::new(),
        DiagnosticSnapshot::Rejected(diagnostics) => diagnostics
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code().as_str().to_owned())
            .collect(),
    };
    let disposition = CompileAttempt::finalize(sealed, None)
        .unwrap()
        .disposition();
    (codes, disposition)
}

#[test]
fn unused_type_applications_cannot_skip_declared_bounds() {
    for source in [
        "trait Mark {}\nstruct Guarded[T: Mark] {}\nfn take(x: Guarded[i64]) -> Unit = ()\n",
        "trait Mark {}\nenum Guarded[T: Mark] { Item(T) }\nfn take(x: Guarded[i64]) -> Unit = ()\n",
        "trait Mark {}\ntype Checked[T: Mark] = T\nfn take(x: Checked[i64]) -> i64 = x\n",
        "trait Mark {}\ntype Checked[T: Mark] = Unit\nfn take(x: Checked[i64]) -> Unit = x\n",
        "trait Mark {}\ntype Checked[T: Mark] = T\npub fn pass[T](x: Checked[T]) -> T = x\n",
        "trait Mark {}\ntrait Uses[A: Mark] {}\nfn take[T: Uses[i64]](x: T) -> Unit = ()\n",
        "trait Mark {}\ntrait Uses[A: Mark] {}\nstruct Carrier {}\nimpl Uses[i64] for Carrier {}\n",
    ] {
        let (codes, disposition) = compile(source);
        assert_eq!(codes, ["TRAIT_BOUND_NOT_SATISFIED"], "{source}");
        assert_ne!(disposition, CompileDisposition::Succeeded, "{source}");
    }
}

#[test]
fn type_applications_use_concrete_and_complete_symbolic_evidence() {
    for source in [
        "trait Mark {}\nimpl Mark for i64 {}\ntype Checked[T: Mark] = T\nfn take(x: Checked[i64]) -> i64 = x\n",
        "trait Mark {}\ntype Checked[T: Mark] = Unit\nfn take[T: Mark](x: Checked[T]) -> Unit = x\n",
        "trait Mark {}\ntrait Uses[A: Mark] {}\nfn take[T: Uses[A], A: Mark](x: T) -> Unit = ()\n",
        "trait Mark {}\ntrait Uses[A: Mark] {}\nstruct Carrier {}\nimpl Mark for i64 {}\nimpl Uses[i64] for Carrier {}\n",
        "trait Carries { type Item }\nstruct Carrier {}\nimpl Carries for Carrier { type Item = i64 }\ntype Item[T: Carries] = T::Item\nfn take(x: Item[Carrier]) -> i64 = x\n",
    ] {
        let (codes, disposition) = compile(source);
        assert!(codes.is_empty(), "{source}: {codes:?}");
        assert_eq!(disposition, CompileDisposition::Succeeded, "{source}");
    }
}

#[test]
fn bounds_are_checked_in_nested_and_non_callable_declarations() {
    for declaration in [
        "struct Owner { field value: Guarded[i64] }",
        "enum Choice { Item(Guarded[i64]) }",
        "enum Choice { Item { value: Guarded[i64] } }",
        "type Bad = Guarded[i64]",
        "type Erase[T] = Unit\ntype Bad = Erase[Guarded[i64]]",
        "type Bad = Fn(Guarded[i64]) -> Unit",
        "fn take(x: Opt[Guarded[i64]]) -> Unit = ()",
        "fn take() -> Unit { let x: Checked[i64] = ()\n return }",
        "fn identity[T](x: T) -> T = x\nfn take() -> Unit = identity::[Checked[i64]](())",
        "trait Carries { type Item }\nstruct Carrier {}\nimpl Carries for Carrier { type Item = Guarded[i64] }",
        "trait Uses {}\nimpl Uses for Guarded[i64] {}",
    ] {
        let source = format!(
            "trait Mark {{}}\nstruct Guarded[T: Mark] {{}}\ntype Checked[T: Mark] = Unit\n{declaration}\n"
        );
        let (codes, disposition) = compile(&source);
        assert_eq!(codes, ["TRAIT_BOUND_NOT_SATISFIED"], "{source}");
        assert_ne!(disposition, CompileDisposition::Succeeded, "{source}");
    }
}

#[test]
fn complete_argument_vectors_and_normalized_assumptions_are_used() {
    for source in [
        "trait Link[A] {}\nstruct Pair[A, B: Link[A]] {}\nfn take[A, B: Link[A]](x: Pair[A, B]) -> Unit = ()\n",
        "trait Link[A] {}\nstruct Pair[A: Link[B], B] {}\nfn take[A: Link[B], B](x: Pair[A, B]) -> Unit = ()\n",
        "trait Mark {}\ntrait Carries { type Item }\nstruct Carrier {}\nimpl Mark for i64 {}\nimpl Carries for Carrier { type Item = i64 }\ntype Item[T: Carries] = T::Item\nstruct Guarded[T: Mark] {}\nfn take(x: Guarded[Item[Carrier]]) -> Unit = ()\n",
        "trait Link[A] {}\ntrait Carries { type Item }\nstruct Carrier {}\nimpl Carries for Carrier { type Item = i64 }\ntype Item[T: Carries] = T::Item\ntype Checked[T: Link[i64]] = Unit\nfn take[T: Link[Item[Carrier]]](x: Checked[T]) -> Unit = x\n",
        "trait Mark {}\nstruct Guarded[T: Mark] {}\nstruct Carrier {}\nimpl Mark for Carrier { fn value(self: &Self, x: Guarded[Self]) -> Unit = () }\n",
    ] {
        let (codes, disposition) = compile(source);
        assert!(codes.is_empty(), "{source}: {codes:?}");
        assert_eq!(disposition, CompileDisposition::Succeeded, "{source}");
    }
}

#[test]
fn missing_wrong_and_recursive_impl_proofs_do_not_admit_applications() {
    for source in [
        "trait Link[A] {}\nstruct Pair[A, B: Link[A]] {}\nfn take[A, B: Link[Bool]](x: Pair[A, B]) -> Unit = ()\n",
        "trait Mark {}\nimpl[T: Mark] Mark for T {}\nstruct Guarded[T: Mark] {}\nfn take(x: Guarded[i64]) -> Unit = ()\n",
        "trait Mark {}\ntrait Carries { type Item }\nstruct Carrier {}\nimpl Carries for Carrier { type Item = i64 }\ntype Item[T: Carries] = T::Item\nstruct Guarded[T: Mark] {}\nfn take(x: Guarded[Item[Carrier]]) -> Unit = ()\n",
    ] {
        let (codes, disposition) = compile(source);
        assert_eq!(codes, ["TRAIT_BOUND_NOT_SATISFIED"], "{source}");
        assert_ne!(disposition, CompileDisposition::Succeeded, "{source}");
    }
}

#[test]
fn structural_errors_precede_application_bounds() {
    for (argument, expected) in [
        ("i64, Bool", "TYPE_ARG_ARITY_MISMATCH"),
        ("Missing", "NAME_NOT_FOUND"),
        ("Own[i64]", "OWNERSHIP_TYPE_POSITION_FORBIDDEN"),
        ("&i64", "REFERENCE_TYPE_POSITION_FORBIDDEN"),
    ] {
        let source = format!(
            "trait Mark {{}}\ntype Checked[T: Mark] = Unit\nfn take(x: Checked[{argument}]) -> Unit = ()\n"
        );
        let (codes, _) = compile(&source);
        assert_eq!(codes, [expected], "{source}");
    }
}

#[test]
fn invalid_alias_declarations_poison_expanded_consumers_without_cascades() {
    for declarations in [
        "type Bad = Checked[i64]\nfn take(x: Bad) -> Bool = x",
        "fn take(x: Bad) -> Bool = x\ntype Bad = Checked[i64]",
    ] {
        let source = format!("trait Mark {{}}\ntype Checked[T: Mark] = Unit\n{declarations}\n");
        let (codes, _) = compile(&source);
        assert_eq!(codes, ["TRAIT_BOUND_NOT_SATISFIED"], "{source}");
    }
}

#[test]
fn recursive_impl_bounds_normalize_substituted_projections() {
    let source = concat!(
        "trait HasItem { type Item }\ntrait Uses[A] {}\ntrait Mark {}\n",
        "struct Carrier {}\nstruct Wrap[T] {}\n",
        "impl HasItem for Carrier { type Item = i64 }\n",
        "impl Uses[i64] for Carrier {}\n",
        "impl[T: HasItem + Uses[T::Item]] Mark for Wrap[T] {}\n",
        "type Checked[T: Mark] = Unit\nfn take(x: Checked[Wrap[Carrier]]) -> Unit = x\n",
    );
    let (codes, disposition) = compile(source);
    assert!(codes.is_empty(), "{codes:?}");
    assert_eq!(disposition, CompileDisposition::Succeeded);
}

#[test]
fn independent_binding_and_method_failures_remain_roots() {
    let source = concat!(
        "trait Mark {}\ntrait Carries { type Item\nfn get(self: &Self) -> Self::Item }\n",
        "struct Carrier {}\nimpl Carries for Carrier { type Item = i64 }\n",
        "type Item[T: Carries] = T::Item\nstruct Guarded[T: Mark] {}\n",
        "fn take(x: Guarded[Item[Carrier]]) -> Unit = ()\n",
    );
    let (codes, _) = compile(source);
    assert_eq!(
        codes,
        ["TRAIT_IMPL_MISSING_METHOD", "TRAIT_BOUND_NOT_SATISFIED"]
    );
}

#[test]
fn diagnostics_preserve_written_bound_priority_and_the_complete_application_span() {
    for (bounds, expected) in [("Zulu + Alpha", "Zulu"), ("Alpha + Zulu", "Alpha")] {
        let source = format!(
            "trait Alpha {{}}\ntrait Zulu {{}}\ntype Checked[T: {bounds}] = Unit\nfn take(x: Checked[i64]) -> Unit = x\n"
        );
        let mut session = session(&source);
        let catalog = TrustedCatalog::first_slice().unwrap();
        let analysis = session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert!(analysis.fingerprints().is_none());
        assert!(analysis.resolution().unwrap().fingerprints().is_none());
        let DiagnosticSnapshot::Rejected(snapshot) = session.into_diagnostics(None) else {
            panic!("missing bound diagnostic")
        };
        assert_eq!(snapshot.diagnostics().len(), 1);
        let diagnostic = &snapshot.diagnostics()[0];
        assert_eq!(
            diagnostic.data().get("trait"),
            Some(&DiagnosticDataValue::from(expected))
        );
        assert_eq!(
            diagnostic.data().get("type"),
            Some(&DiagnosticDataValue::from("i64"))
        );
        let span = diagnostic
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range();
        assert_eq!(
            source[span.start().as_usize()..span.end().as_usize()].trim(),
            "Checked[i64]"
        );
    }
}

#[test]
fn invalid_impl_applications_cannot_survive_as_checked_evidence() {
    for declarations in [
        "type Checked[T: Mark] = Unit\nfn take(x: Checked[i64]) -> Bool = x\nimpl Mark for i64 { type Item = Guarded[Bool] }",
        "impl Mark for i64 { type Item = Guarded[Bool] }\ntype Checked[T: Mark] = Unit\nfn take(x: Checked[i64]) -> Bool = x",
    ] {
        let source = format!(
            "trait Needed {{}}\ntrait Mark {{ type Item }}\nstruct Guarded[T: Needed] {{}}\n{declarations}\n"
        );
        let (codes, _) = compile(&source);
        assert_eq!(codes, ["TRAIT_BOUND_NOT_SATISFIED"], "{source}");
    }
}

#[test]
fn a_late_impl_method_failure_invalidates_provisional_application_success() {
    let source = concat!(
        "trait Mark {}\nimpl Mark for i64 {}\n",
        "trait Carries { type Item\nfn get(self: &Self) -> Self::Item }\n",
        "struct Carrier {}\nimpl Carries for Carrier { type Item = i64 }\n",
        "type Item[T: Carries] = T::Item\nstruct Guarded[T: Mark] {}\n",
        "fn take(x: Guarded[Item[Carrier]]) -> Bool = x\n",
    );
    let mut session = session(source);
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().unwrap();
    let take = resolution
        .declarations()
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| {
            declaration
                .name()
                .is_some_and(|name| name.spelling() == "take")
        })
        .unwrap();
    assert!(matches!(
        resolution.signatures().source_declaration(take.id()),
        eglur_sema::FactState::Poisoned(_)
    ));
    assert!(resolution.fingerprints().is_none());
    let DiagnosticSnapshot::Rejected(snapshot) = session.into_diagnostics(None) else {
        panic!("incomplete impl must fail")
    };
    assert_eq!(
        snapshot
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code().as_str())
            .collect::<Vec<_>>(),
        ["TRAIT_IMPL_MISSING_METHOD"]
    );
}

#[test]
fn conflicting_impls_do_not_add_a_derivative_bound_error() {
    let source = concat!(
        "trait Mark {}\nimpl Mark for i64 {}\nimpl Mark for i64 {}\n",
        "type Checked[T: Mark] = Unit\nfn take(x: Checked[i64]) -> Unit = x\n",
    );
    let (codes, disposition) = compile(source);
    assert_eq!(codes, ["TRAIT_IMPL_DUPLICATE"]);
    assert_eq!(disposition, CompileDisposition::Rejected);
}

#[test]
fn application_assumptions_do_not_leak_between_owners() {
    let source = concat!(
        "trait Mark {}\ntype Checked[T: Mark] = Unit\n",
        "fn yes[T: Mark](x: Checked[T]) -> Unit = x\n",
        "fn no[T](x: Checked[T]) -> Unit = x\n",
    );
    let (codes, disposition) = compile(source);
    assert_eq!(codes, ["TRAIT_BOUND_NOT_SATISFIED"]);
    assert_eq!(disposition, CompileDisposition::Rejected);
}

#[test]
fn an_invalid_owner_bound_is_not_blamed_on_an_unrelated_application() {
    let source = concat!(
        "trait Mark {}\ntrait Carries { type Item }\ntrait Uses[A] {}\n",
        "struct Carrier {}\ntype Item[T: Carries] = T::Item\n",
        "fn take[A: Mark, B: Uses[Item[Carrier]]](a: A, b: B) -> Unit = ()\n",
    );
    let mut session = session(source);
    let catalog = TrustedCatalog::first_slice().unwrap();
    session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let DiagnosticSnapshot::Rejected(snapshot) = session.into_diagnostics(None) else {
        panic!("missing bound diagnostic")
    };
    assert_eq!(snapshot.diagnostics().len(), 1);
    let diagnostic = &snapshot.diagnostics()[0];
    assert_eq!(diagnostic.code().as_str(), "TRAIT_BOUND_NOT_SATISFIED");
    let span = diagnostic
        .primary_span()
        .as_source()
        .unwrap()
        .file_span()
        .range();
    assert_eq!(
        source[span.start().as_usize()..span.end().as_usize()].trim(),
        "Uses[Item[Carrier]]"
    );
}

#[test]
fn owner_assumption_normalization_is_independent_of_parameter_order() {
    for parameters in [
        "U: Link[DerivedItem[Wrap[T]]], T: Uses[Item[Carrier]]",
        "T: Uses[Item[Carrier]], U: Link[DerivedItem[Wrap[T]]]",
    ] {
        let source = format!(
            "trait Carries {{ type Item }}\ntrait Uses[A] {{}}\ntrait Derived {{ type Item }}\ntrait Link[A] {{}}\n\
             struct Carrier {{}}\nstruct Wrap[T] {{}}\n\
             impl Carries for Carrier {{ type Item = i64 }}\n\
             impl[T: Uses[i64]] Derived for Wrap[T] {{ type Item = Bool }}\n\
             type Item[T: Carries] = T::Item\ntype DerivedItem[T: Derived] = T::Item\n\
             type Checked[T: Link[Bool]] = Unit\nfn take[{parameters}](x: Checked[U]) -> Unit = x\n"
        );
        let (codes, disposition) = compile(&source);
        assert!(codes.is_empty(), "{source}: {codes:?}");
        assert_eq!(disposition, CompileDisposition::Succeeded);
    }
}
