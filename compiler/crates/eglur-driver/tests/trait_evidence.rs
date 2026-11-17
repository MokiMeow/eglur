use eglur_diagnostics::{DiagnosticDataValue, DiagnosticSnapshot};
use eglur_driver::{
    CompilerSession, FileInput, FileSetRequest, SessionOptions, SupportProfile, SupportProfileId,
    SupportSchemaVersion, TrustedCatalog,
};
use eglur_hir::ItemKind;
use eglur_sema::{
    CanonicalTarget, CheckedDispatch, CheckedInvocation, CompileCapability, DeclarationKind,
    EvidenceSource, FactState, InvocationTarget, PhaseProduct, ResolveCapability,
    TypecheckCapability,
};

#[path = "trait_evidence/missing_methods.rs"]
mod missing_methods;

fn trait_profile() -> SupportProfile {
    trait_profile_with_evidence(true)
}

fn trait_profile_with_evidence(include_evidence: bool) -> SupportProfile {
    let mut capabilities = vec![
        CompileCapability::Resolve(ResolveCapability::CollectDeclarations),
        CompileCapability::Resolve(ResolveCapability::ResolveFileSetImports),
        CompileCapability::Resolve(ResolveCapability::ResolveLexicalValues),
        CompileCapability::Resolve(ResolveCapability::ResolveModuleValues),
        CompileCapability::Resolve(ResolveCapability::ResolveQualifiedValues),
        CompileCapability::Resolve(ResolveCapability::ResolvePatternScopes),
        CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames),
        CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures),
        CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEnvironment),
        CompileCapability::Typecheck(TypecheckCapability::CheckBodies),
        CompileCapability::Typecheck(TypecheckCapability::ResolveMembers),
        CompileCapability::Typecheck(TypecheckCapability::ResolveMethods),
    ];
    if include_evidence {
        capabilities.push(CompileCapability::Typecheck(
            TypecheckCapability::ResolveTraitEvidence,
        ));
    }
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new(if include_evidence {
            "trait-evidence-v1"
        } else {
            "trait-body-only-v1"
        })
        .unwrap(),
        capabilities,
    )
    .unwrap()
}

#[test]
fn unsatisfied_inferred_bound_reports_the_required_contract_data() {
    let mut session = session(concat!(
        "trait Mark {}\n",
        "struct Plain {}\n",
        "fn bounded[T: Mark](value: T) -> T = value\n",
        "fn invalid(value: Plain) -> Plain = bounded(value)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code().as_str(), "TRAIT_BOUND_NOT_SATISFIED");
    assert_eq!(
        diagnostics[0].data().get("trait"),
        Some(&DiagnosticDataValue::from("Mark"))
    );
    assert_eq!(
        diagnostics[0].data().get("type"),
        Some(&DiagnosticDataValue::from("Plain"))
    );
}

#[test]
fn unselected_evidence_phase_preserves_body_facts_without_claiming_dispatch() {
    let mut session = session(concat!(
        "fn identity[T](value: T) -> T = value\n",
        "fn main() -> i64 = identity(1)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session
        .resolve(&trait_profile_with_evidence(false), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().expect("body typing still resolves");

    assert!(matches!(
        resolution.trait_evidence(),
        PhaseProduct::Unperformed
    ));
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability()
            == CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEvidence)
    }));
}

fn session(source: &str) -> CompilerSession {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    CompilerSession::build(request, SessionOptions::default()).unwrap()
}

#[test]
fn malformed_inline_bounds_keep_their_syntax_root_through_semantic_analysis() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (bound, head) in [
        ("&Show", "&"),
        ("&mut Show", "&"),
        ("*const Show", "*"),
        ("*mut Show", "*"),
        ("(Show)", "("),
        ("(Show, Other)", "("),
        ("Fn(i64) -> Unit", "Fn"),
        ("dyn Show", "dyn"),
    ] {
        let text = format!(
            "trait Show {{}}\ntrait Other {{}}\nfn broken[T: {bound}]() -> Unit = ()\nfn following() -> Unit = ()\n"
        );
        let mut session = session(&text);
        let source = &session.sources()[0];
        assert!(!source.parse().unwrap().is_success(), "{text}");
        let hir = source.hir().unwrap();
        assert!(!hir.invalid_occurrences().is_empty(), "{text}");
        assert_eq!(
            source.invalid_facts().len(),
            hir.invalid_occurrences().len()
        );
        assert!(
            source
                .invalid_facts()
                .all(|fact| !fact.occurrence().syntax_causes().is_empty())
        );
        for (cause, _) in hir.syntax_causes() {
            assert!(source.syntax_poison(cause).is_some());
        }
        let ItemKind::Function(following) = hir.item(*hir.roots().last().unwrap()).kind() else {
            panic!("the following function must survive recovery: {text}");
        };
        assert_eq!(following.name.text(), Some("following"));

        session.resolve(&trait_profile(), Some(&catalog)).unwrap();
        let snapshot = session.into_diagnostics(None);
        let diagnostics = snapshot.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{text}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code().as_str(), "PARSE_UNEXPECTED_TOKEN");
        let span = diagnostics[0]
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range();
        assert_eq!(span.start().as_usize(), text.find(bound).unwrap());
        assert_eq!(&text[span.start().as_usize()..span.end().as_usize()], head);
    }
}

#[test]
fn non_trait_and_duplicate_bound_paths_remain_semantic_errors() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for text in [
        "struct Plain {}\nfn sample[T: Plain](value: T) -> T = value\n",
        "trait Show {}\nfn sample[T: Show + Show](value: T) -> T = value\n",
    ] {
        let mut session = session(text);
        let source = &session.sources()[0];
        assert!(source.parse().unwrap().is_success(), "{text}");
        assert!(source.hir().unwrap().invalid_occurrences().is_empty());

        session.resolve(&trait_profile(), Some(&catalog)).unwrap();
        let snapshot = session.into_diagnostics(None);
        let diagnostics = snapshot.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{text}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code().as_str(), "TRAIT_BOUND_INVALID");
    }
}

#[test]
fn trait_signatures_distinguish_closed_names_from_closed_modifiers() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (source, expected_closed) in [
        ("trait closed {}\n", false),
        ("pub trait closed[T] {}\n", false),
        ("closed trait closed {}\n", true),
        ("pub closed trait closed[T] {}\n", true),
    ] {
        let mut session = session(source);
        let analysis = session.resolve(&trait_profile(), Some(&catalog)).unwrap();
        let resolution = analysis.resolution().expect("trait profile resolves");
        let declaration = resolution
            .declarations()
            .modules()
            .flat_map(|module| module.declarations())
            .find(|declaration| declaration.kind() == DeclarationKind::Trait)
            .expect("source declares one trait");
        let FactState::Known(eglur_sema::DeclarationSignature::Trait { closed, .. }) =
            resolution.signatures().source_declaration(declaration.id())
        else {
            panic!("source trait signature should be known");
        };

        assert_eq!(*closed, expected_closed, "{source}");
        assert!(matches!(
            session.into_diagnostics(None),
            DiagnosticSnapshot::Accepted(_)
        ));
    }
}

fn file(path: &str, source: &str) -> FileInput {
    FileInput::new(path, source.as_bytes().to_vec())
}

#[test]
fn generic_bounds_materialize_bound_and_impl_evidence() {
    let mut session = session(concat!(
        "fn show[T: Display](value: T) -> Str = to_str(value)\n",
        "fn main() -> Str = show(1)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("trait profile resolves");
    let PhaseProduct::Performed(evidence) = resolution.trait_evidence() else {
        panic!("trait evidence must be performed")
    };
    let sources = evidence
        .environments()
        .flat_map(|environment| environment.entries())
        .map(|entry| evidence.proof(entry.proof()).expect("entry proof exists"))
        .collect::<Vec<_>>();

    assert!(
        sources
            .iter()
            .any(|source| matches!(source, EvidenceSource::Bound(_)))
    );
    assert!(
        sources
            .iter()
            .any(|source| matches!(source, EvidenceSource::Implementation { .. }))
    );
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn method_dispatch_distinguishes_witnesses_and_impl_templates() {
    let mut session = session(concat!(
        "trait Readable {\n",
        "  type Elem\n",
        "  fn read(self: &Self) -> Self::Elem\n",
        "}\n",
        "struct Count {}\n",
        "impl Readable for Count {\n",
        "  type Elem = i64\n",
        "  fn read(self: &Self) -> i64 = 1\n",
        "}\n",
        "fn project[C: Readable](value: C) -> C::Elem = value.read()\n",
        "fn direct(value: Count) -> i64 = value.read()\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("trait profile resolves");
    let PhaseProduct::Performed(evidence) = resolution.trait_evidence() else {
        panic!("trait evidence must be performed")
    };
    let dispatches = evidence
        .invocations()
        .filter_map(|(_, fact)| match fact {
            FactState::Known(CheckedInvocation::Static { dispatch, .. }) => Some(dispatch),
            FactState::Known(CheckedInvocation::Indirect { .. }) => None,
            FactState::Poisoned(_) | FactState::NotApplicable | FactState::Unperformed => None,
        })
        .collect::<Vec<_>>();

    assert!(
        dispatches
            .iter()
            .any(|dispatch| matches!(dispatch, CheckedDispatch::Bound { .. }))
    );
    assert!(
        dispatches
            .iter()
            .any(|dispatch| matches!(dispatch, CheckedDispatch::ImplTemplate { .. }))
    );
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn imported_traits_contribute_methods_but_unimported_traits_do_not() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let library = file(
        "library.eglur",
        concat!(
            "pub struct Doc {}\n",
            "pub trait Render { fn render(self: &Self) -> Str }\n",
            "impl Render for Doc { fn render(self: &Self) -> Str = \"ok\" }\n",
        ),
    );
    let imported = file(
        "main.eglur",
        concat!(
            "import library::Doc\n",
            "import library::Render\n",
            "fn show(value: Doc) -> Str = value.render()\n",
        ),
    );
    let request =
        FileSetRequest::new([library.clone(), imported], "main.eglur", Some(".")).unwrap();
    let mut accepted = CompilerSession::build(request, SessionOptions::default()).unwrap();
    accepted.resolve(&trait_profile(), Some(&catalog)).unwrap();
    assert!(matches!(
        accepted.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));

    let unimported = file(
        "main.eglur",
        concat!(
            "import library::Doc\n",
            "fn show(value: Doc) -> Str = value.render()\n",
        ),
    );
    let request = FileSetRequest::new([library, unimported], "main.eglur", Some(".")).unwrap();
    let mut rejected = CompilerSession::build(request, SessionOptions::default()).unwrap();
    rejected.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let diagnostics = rejected.into_diagnostics(None);
    assert_eq!(
        diagnostics
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code().as_str())
            .collect::<Vec<_>>(),
        ["METHOD_NOT_FOUND"]
    );
}

#[test]
fn local_trait_name_shadows_trusted_prelude_method_visibility() {
    let mut session = session(concat!(
        "trait Display {}\n",
        "fn show(value: i64) -> Str = value.to_str()\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let diagnostics = session.into_diagnostics(None);
    assert_eq!(diagnostics.diagnostics().len(), 1);
    assert_eq!(
        diagnostics.diagnostics()[0].code().as_str(),
        "METHOD_NOT_FOUND"
    );
}

#[test]
fn ambiguous_visible_trait_methods_require_a_fully_qualified_call() {
    let mut session = session(concat!(
        "trait Left { fn read(self: &Self) -> i64 }\n",
        "trait Right { fn read(self: &Self) -> i64 }\n",
        "struct Count {}\n",
        "impl Left for Count { fn read(self: &Self) -> i64 = 1 }\n",
        "impl Right for Count { fn read(self: &Self) -> i64 = 2 }\n",
        "fn use(value: Count) -> i64 = value.read()\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code().as_str(), "TRAIT_METHOD_AMBIGUOUS");
    assert!(diagnostics[0].message().contains("fully qualified"));
    assert_eq!(
        diagnostics[0].data().get("name"),
        Some(&DiagnosticDataValue::from("read"))
    );
    assert_eq!(diagnostics[0].related().len(), 2);
}

#[test]
fn required_trait_method_omission_is_rejected_at_the_impl() {
    let mut session = session(concat!(
        "trait Readable { fn read(self: &Self) -> i64 }\n",
        "struct Count {}\n",
        "impl Readable for Count {}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code().as_str(), "TRAIT_IMPL_MISSING_METHOD");
    assert_eq!(
        diagnostics[0].data().get("trait"),
        Some(&DiagnosticDataValue::from("Readable"))
    );
    assert_eq!(
        diagnostics[0].data().get("method"),
        Some(&DiagnosticDataValue::from("read"))
    );
    assert_eq!(diagnostics[0].related().len(), 1);
    assert_eq!(
        diagnostics[0].related()[0].label(),
        "trait method declared here"
    );
}

#[test]
fn trusted_trait_methods_apply_the_same_source_impl_contract() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut missing = session(concat!("struct Label {}\n", "impl Display for Label {}\n",));
    missing.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let diagnostics = missing.into_diagnostics(None);
    assert_eq!(diagnostics.diagnostics().len(), 1);
    assert_eq!(
        diagnostics.diagnostics()[0].code().as_str(),
        "TRAIT_IMPL_MISSING_METHOD"
    );
    assert_eq!(diagnostics.diagnostics()[0].related().len(), 0);

    let mut mismatched = session(concat!(
        "struct Label {}\n",
        "impl Display for Label {\n",
        "  fn to_str(self: &Self) -> i64 = 0\n",
        "}\n",
    ));
    mismatched
        .resolve(&trait_profile(), Some(&catalog))
        .unwrap();
    let diagnostics = mismatched.into_diagnostics(None);
    assert_eq!(diagnostics.diagnostics().len(), 1);
    assert_eq!(
        diagnostics.diagnostics()[0].code().as_str(),
        "TRAIT_IMPL_SIGNATURE_MISMATCH"
    );
    assert!(
        diagnostics.diagnostics()[0].data()["expected_signature"]
            .to_string()
            .contains("fn to_str(self: &Label) -> Str")
    );
}

#[test]
fn otherwise_matching_trait_and_impl_methods_must_share_bangness() {
    let mut session = session(concat!(
        "trait Runnable { fn run!(self: Self) -> Unit }\n",
        "struct Job {}\n",
        "impl Runnable for Job { fn run(self: Self) -> Unit = () }\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code().as_str(), "TRAIT_EFFECT_MISMATCH");
    assert_eq!(
        diagnostics[0].data().get("trait"),
        Some(&DiagnosticDataValue::from("Runnable"))
    );
    assert_eq!(
        diagnostics[0].data().get("method"),
        Some(&DiagnosticDataValue::from("run!"))
    );
    assert!(diagnostics[0].data().contains_key("expected_signature"));
    assert!(diagnostics[0].data().contains_key("actual_signature"));
    assert_eq!(diagnostics[0].related().len(), 1);
}

#[test]
fn trait_impl_method_signature_mismatch_exposes_both_contracts() {
    let mut session = session(concat!(
        "trait Render { fn render(self: &Self, value: i64) -> Str }\n",
        "struct Doc {}\n",
        "impl Render for Doc {\n",
        "  fn render(self: &Self, value: f64) -> Str = \"bad\"\n",
        "}\n",
        "fn use(value: Doc) -> Str = value.render(1)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].code().as_str(),
        "TRAIT_IMPL_SIGNATURE_MISMATCH"
    );
    assert_eq!(
        diagnostics[0].data().get("trait"),
        Some(&DiagnosticDataValue::from("Render"))
    );
    assert_eq!(
        diagnostics[0].data().get("method"),
        Some(&DiagnosticDataValue::from("render"))
    );
    assert!(diagnostics[0].data().contains_key("expected_signature"));
    assert!(diagnostics[0].data().contains_key("actual_signature"));
    assert_eq!(diagnostics[0].related().len(), 1);
}

#[test]
fn trait_impl_method_generic_bounds_are_alpha_normalized_and_order_insensitive() {
    let mut session = session(concat!(
        "trait Left {}\n",
        "trait Right {}\n",
        "trait Mapper {\n",
        "  fn map[T: Left + Right](self: &Self, value: T) -> T\n",
        "}\n",
        "struct Host {}\n",
        "impl Mapper for Host {\n",
        "  fn map[U: Right + Left](self: &Self, value: U) -> U = value\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&trait_profile(), Some(&catalog)).unwrap();

    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn trait_impl_method_generic_bound_set_difference_is_rejected() {
    let mut session = session(concat!(
        "trait Left {}\n",
        "trait Extra {}\n",
        "trait Mapper { fn map[T: Left](self: &Self, value: T) -> T }\n",
        "struct Host {}\n",
        "impl Mapper for Host {\n",
        "  fn map[U: Left + Extra](self: &Self, value: U) -> U = value\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].code().as_str(),
        "TRAIT_IMPL_SIGNATURE_MISMATCH"
    );
}

#[test]
fn omitted_default_method_dispatches_to_the_trait_callable() {
    let mut session = session(concat!(
        "trait Readable { fn read(self: &Self) -> i64 = 7 }\n",
        "struct Count {}\n",
        "impl Readable for Count {}\n",
        "fn read_count(value: Count) -> i64 = value.read()\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("trait profile resolves");
    let PhaseProduct::Performed(evidence) = resolution.trait_evidence() else {
        panic!("trait evidence must be performed")
    };
    let callable = evidence
        .invocations()
        .find_map(|(_, fact)| match fact {
            FactState::Known(CheckedInvocation::Static {
                dispatch: CheckedDispatch::ImplTemplate { callable, .. },
                ..
            }) => Some(callable),
            FactState::Known(_) => None,
            FactState::Poisoned(_) | FactState::NotApplicable | FactState::Unperformed => None,
        })
        .expect("concrete default call has checked impl-template dispatch");
    let CanonicalTarget::Source(callable) = callable else {
        panic!("source trait default has a source callable")
    };
    let method = resolution
        .declarations()
        .declaration(*callable)
        .expect("dispatch target is a declaration");
    let owner = method.parent().expect("trait method has an owner");

    assert_eq!(
        resolution
            .declarations()
            .declaration(owner)
            .expect("trait owner remains available")
            .kind(),
        DeclarationKind::Trait
    );
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn generic_impl_and_method_substitutions_remain_separate_and_complete() {
    let mut session = session(concat!(
        "trait Echo[A] { fn echo[B](self: &Self, value: B) -> B }\n",
        "struct Wrap[T] {}\n",
        "impl[T] Echo[T] for Wrap[T] {\n",
        "  fn echo[B](self: &Self, value: B) -> B = value\n",
        "}\n",
        "fn use(value: Wrap[i64]) -> Str = value.echo(\"ok\")\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("trait profile resolves");
    let PhaseProduct::Performed(evidence) = resolution.trait_evidence() else {
        panic!("trait evidence must be performed")
    };
    let (key, impl_substitution) = evidence
        .invocations()
        .find_map(|(key, fact)| match fact {
            FactState::Known(CheckedInvocation::Static {
                dispatch: CheckedDispatch::ImplTemplate { proof, .. },
                ..
            }) => {
                let Some(EvidenceSource::Implementation { substitution, .. }) =
                    evidence.proof(*proof)
                else {
                    panic!("impl-template proof selects one implementation");
                };
                Some((key, *substitution))
            }
            FactState::Known(_) => None,
            FactState::Poisoned(_) | FactState::NotApplicable | FactState::Unperformed => None,
        })
        .expect("generic trait call has checked impl dispatch");
    let FactState::Known(invocation) = resolution.body_types().invocation(key) else {
        panic!("checked dispatch retains its invocation fact")
    };
    let InvocationTarget::Callable(instance) = invocation.target() else {
        panic!("method dispatch retains a callable instance")
    };
    let method_substitution = resolution
        .body_types()
        .callable_instance(*instance)
        .expect("callable instance remains available")
        .substitution();

    assert_eq!(
        resolution
            .signatures()
            .types()
            .substitution(impl_substitution)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        resolution
            .signatures()
            .types()
            .substitution(method_substitution)
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn recursive_blanket_obligation_terminates_without_counting_the_cycle_as_proof() {
    let mut session = session(concat!(
        "trait Loop[T] {}\n",
        "impl[T: Loop[T]] Loop[T] for T {}\n",
        "fn require[X: Loop[i64]](value: X) -> X = value\n",
        "fn invalid(value: i64) -> i64 = require(value)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&trait_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code().as_str(), "TRAIT_BOUND_NOT_SATISFIED");
    assert_eq!(
        diagnostics[0].data().get("trait"),
        Some(&DiagnosticDataValue::from("Loop[i64]"))
    );
    assert_eq!(
        diagnostics[0].data().get("type"),
        Some(&DiagnosticDataValue::from("i64"))
    );
}
