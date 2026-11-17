use eglur_diagnostics::{DiagnosticSnapshot, render_plain};
use eglur_driver::{
    CompilerSession, FileSetRequest, SessionOptions, SupportProfile, TrustedCatalog,
};

#[path = "tensor_constructors/traversal.rs"]
mod traversal;

fn check(source: &str) -> CompilerSession {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::tensor_constructors_v1(), Some(&catalog))
        .unwrap();
    let missing = analysis
        .trace()
        .missing()
        .map(|entry| entry.capability().stable_key())
        .collect::<Vec<_>>();
    assert!(missing.is_empty(), "{source}\nmissing: {missing:?}");
    session
}

fn accept(source: &str) {
    let sealed = check(source).seal(None).unwrap();
    assert!(
        sealed.checked_program().is_some(),
        "{source}\n{:?}",
        sealed.diagnostics()
    );
}

fn reject(source: &str, expected: &str) {
    let snapshot = check(source).into_diagnostics(None);
    let DiagnosticSnapshot::Rejected(rejected) = &snapshot else {
        panic!("expected {expected}: {source}\n{snapshot:?}");
    };
    let codes = rejected
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect::<Vec<_>>();
    assert_eq!(codes, [expected], "{source}\n{}", render_plain(&snapshot));
}

#[test]
fn grouped_dtype_callees_preserve_constructor_identity_and_completion() {
    for callee in ["filled", "(filled)", "((filled))"] {
        accept(&format!(
            "import std::tensor::ones as filled\nimport std::tensor::Tensor\n\
             fn make(n: Nat) -> Tensor[f32] = {callee}[f32](n, 2)\n"
        ));
    }
}

#[test]
fn grouped_dtype_callees_keep_type_and_shadowing_errors() {
    for callee in ["ones", "(ones)", "((ones))"] {
        reject(
            &format!(
                "import std::tensor::ones\nfn make() -> Unit {{ let t = {callee}[Str](2)\nyield () }}\n"
            ),
            "CALL_ARG_TYPE_MISMATCH",
        );
        reject(
            &format!(
                "import std::tensor::ones\nfn make() -> Unit {{ let ones = 0\nlet t = {callee}[f32](2)\nyield () }}\n"
            ),
            "TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN",
        );
    }
}

#[test]
fn grouped_non_name_dtype_callees_are_not_tensor_constructors() {
    for callee in ["(1)", "({ || () })", "((1, 2).0)"] {
        reject(
            &format!("fn make() -> Unit {{ let t = {callee}[f32]()\nyield () }}\n"),
            "TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN",
        );
    }
}

#[test]
fn grouped_dtype_callees_keep_the_tensor_typing_capability_requirement() {
    let source =
        "import std::tensor::ones\nfn make() -> Unit { let t = ((ones))[f32](2)\nyield () }\n";
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert!(analysis.trace().missing().any(|entry| entry.capability()
        == eglur_sema::CompileCapability::Typecheck(
            eglur_sema::TypecheckCapability::CheckTensorConstructors
        )));
    let sealed = session.seal(None).unwrap();
    assert!(sealed.checked_program().is_none());
    assert!(sealed.diagnostics().diagnostics().is_empty());
}

#[test]
fn supported_dtypes_and_dynamic_dimensions_produce_tensor_results() {
    for (dtype, element) in [
        ("bool", "Bool"),
        ("Bool", "Bool"),
        ("i8", "i8"),
        ("i16", "i16"),
        ("i32", "i32"),
        ("i64", "i64"),
        ("u8", "u8"),
        ("u16", "u16"),
        ("u32", "u32"),
        ("u64", "u64"),
        ("f16", "f16"),
        ("bf16", "bf16"),
        ("f32", "f32"),
        ("f64", "f64"),
        ("complex64", "complex64"),
        ("complex128", "complex128"),
    ] {
        for constructor in ["zeros", "ones"] {
            accept(&format!(
                "import std::tensor::{constructor}\nimport std::tensor::Tensor\nfn make(n: Nat) -> Tensor[{element}] = {constructor}[{dtype}](n, 0, 2)\n"
            ));
        }
    }
}

#[test]
fn aliases_and_rank_zero_are_supported() {
    accept(
        "import std::tensor::ones as filled\nimport std::tensor::Tensor\ntype Real = f32\nfn make() -> Tensor[Real] = filled[Real]()\n",
    );
    accept(
        "import std::tensor::zeros\nimport std::tensor::Tensor\nfn make() -> Tensor[Bool] = zeros[(bool)]()\n",
    );
    accept(
        "import std::tensor::nan\nimport std::tensor::Tensor\nfn make() -> Tensor[f32] = nan[f32](1)\n",
    );
    accept(
        "import std::tensor::rand!\nimport std::tensor::Tensor\nfn make!() -> Tensor[f32] = rand![f32](1)\n",
    );
}

#[test]
fn unsupported_dtypes_and_signed_dimensions_are_rejected() {
    for call in ["ones[Str](2)", "nan[f64](2)", "rand![f64](2)"] {
        let name = call.split('[').next().unwrap();
        reject(
            &format!(
                "import std::tensor::{name}\nfn make!() -> Unit {{ let value = {call}\nyield () }}\n"
            ),
            "CALL_ARG_TYPE_MISMATCH",
        );
    }
    reject(
        "import std::tensor::ones\nfn make(n: i64) -> Unit { let value = ones[f32](n)\nyield () }\n",
        "CALL_ARG_TYPE_MISMATCH",
    );
}

#[test]
fn constructor_errors_keep_type_data_and_nat_boundaries() {
    let snapshot =
        check("import std::tensor::nan\nfn make!() -> Unit { let t = nan[f64](2)\nyield () }\n")
            .into_diagnostics(None);
    let data = snapshot.diagnostics()[0].data();
    assert_eq!(data.get("reason"), Some(&"type_family".into()));
    assert_eq!(data.get("actual_type"), Some(&"f64".into()));
    assert_eq!(
        data.get("expected_family"),
        Some(&"constructor_dtype".into())
    );
    reject(
        "import std::tensor::ones\nfn make() -> Unit { let t = ones[f32](9223372036854775808)\nyield () }\n",
        "DOMAIN_INVALID_VALUE",
    );
    reject(
        "import std::tensor::ones\nimport std::tensor::Tensor\nfn make() -> Tensor[f64] = ones[f32](2)\n",
        "RETURN_TYPE_MISMATCH",
    );
    reject(
        "import std::tensor::ones\nstruct Other {}\nfn make() -> Unit { let t = ones[Other](2)\nyield () }\n",
        "CALL_ARG_TYPE_MISMATCH",
    );
}

#[test]
fn random_and_argument_effects_cannot_hide_in_pure_constructors() {
    reject(
        "import std::tensor::rand!\nfn make() -> Unit { let t = rand![f32](1)\nyield () }\n",
        "EFFECT_VIOLATION",
    );
    reject(
        "import std::tensor::ones\nimport std::test::emit!\nfn dim!() -> Nat { emit!(\"dimension\")\nyield 2 }\nfn make() -> Unit { let t = ones[f32](dim!())\nyield () }\n",
        "EFFECT_VIOLATION",
    );
    let session = check(
        "import std::tensor::rand!\nimport std::tensor::Tensor\nfn make!() -> Tensor[f32] = rand![f32](1)\n",
    );
    let analysis = session.resolution_analysis().unwrap();
    let resolution = analysis.resolution().unwrap();
    let eglur_sema::PhaseProduct::Performed(effects) = resolution.effect_facts() else {
        panic!("missing effects");
    };
    let expected = eglur_sema::EffectSummary::Known(
        eglur_sema::KnownEffectSummary::new(
            [eglur_sema::EffectCategory::Nondet],
            eglur_sema::StateScope::None,
        )
        .unwrap(),
    );
    assert!(effects.summaries().any(|(target, fact)| {
        matches!(target, eglur_sema::CanonicalTarget::Source(_))
            && matches!(fact, eglur_sema::FactState::Known(summary) if summary == &expected)
    }));
}

#[test]
fn checked_constructors_are_refused_before_execution() {
    for callee in ["ones", "((ones))"] {
        let source = format!(
            "import std::tensor::ones\nimport std::test::emit!\nfn main!() -> Unit {{ emit!(\"must not execute\")\nlet t = {callee}[f32](2)\nyield () }}\n"
        );
        let catalog = TrustedCatalog::first_slice().unwrap();
        let profile = SupportProfile::tensor_constructors_v1();
        let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
        let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
        session
            .resolve_for(
                eglur_driver::ProgramMode::Executable,
                &profile,
                Some(&catalog),
            )
            .unwrap();
        let sealed = session.seal(None).unwrap();
        let program = sealed
            .checked_program()
            .expect("constructors statically check");
        assert_eq!(
            eglur_driver::execute(
                program,
                &profile,
                &catalog,
                eglur_driver::EvaluationLimits::default()
            ),
            eglur_driver::ExecutionAttempt::Unsupported(eglur_driver::SupportReason::Feature(
                eglur_driver::UnsupportedFeature::Tensor
            ))
        );
    }
}

#[test]
fn tensor_fingerprints_track_dtype_dimensions_and_constructor_identity() {
    fn fingerprint(call: &str) -> eglur_sema::SemanticFingerprints {
        let source = format!(
            "import std::tensor::ones\nimport std::tensor::zeros\nfn make() -> Unit {{ let t = {call}\nyield () }}\n"
        );
        let session = check(&source);
        session
            .resolution_analysis()
            .unwrap()
            .resolution()
            .unwrap()
            .fingerprints()
            .unwrap()
            .clone()
    }
    let base = fingerprint("ones[f32](2, 3)");
    assert_eq!(base, fingerprint("ones[f32](2, 3)"));
    for call in ["ones[f64](2, 3)", "ones[f32](3, 2)", "zeros[f32](2, 3)"] {
        assert_ne!(base, fingerprint(call), "{call}");
    }
}

#[test]
fn reexports_preserve_identity_but_local_and_source_shadows_do_not() {
    let files = [
        eglur_driver::FileInput::new("bridge.eglur", b"pub import std::tensor::ones as filled\n".to_vec()),
        eglur_driver::FileInput::new("main.eglur", b"import bridge::filled\nimport std::tensor::Tensor\nfn make(n: Nat) -> Tensor[f32] = filled[f32](n)\n".to_vec()),
    ];
    let request = FileSetRequest::new(files, "main.eglur", Some(".")).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    session
        .resolve(
            &SupportProfile::tensor_constructors_v1(),
            Some(&TrustedCatalog::first_slice().unwrap()),
        )
        .unwrap();
    assert!(session.seal(None).unwrap().checked_program().is_some());
    for source in [
        "import std::tensor::ones\nfn ones(n: Nat) -> Nat = n\nfn make() -> Unit { let t = ones[f32](2)\nyield () }\n",
        "import std::tensor::ones\nfn make() -> Unit { let ones = 0\nlet t = ones[f32](2)\nyield () }\n",
    ] {
        reject(source, "TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN");
    }
    reject("fn make(x: bool) -> Unit = ()\n", "NAME_NOT_FOUND");
}

#[test]
fn dimension_invocations_preserve_source_order_and_divergence() {
    let source = "import std::tensor::ones\nfn first() -> Nat = 2\nfn second() -> Nat = 3\nfn make() -> Unit { let t = ones[f32](first(), second())\nyield () }\n";
    let session = check(source);
    let resolution = session.resolution_analysis().unwrap().resolution().unwrap();
    let (_, invocation) = resolution
        .body_types()
        .invocations()
        .find(|(_, invocation)| {
            matches!(
                invocation.target(),
                eglur_sema::InvocationTarget::TensorConstructor { .. }
            )
        })
        .unwrap();
    assert_eq!(invocation.arguments().len(), 2);
    assert_eq!(
        invocation.arguments().iter().collect::<Vec<_>>(),
        invocation.evaluation_order().collect::<Vec<_>>()
    );
    accept(
        "import std::tensor::ones\nimport std::tensor::Tensor\nfn stop() -> Never = stop()\nfn make() -> Tensor[f32] = ones[f32](stop(), { return ones[f32](2) })\n",
    );
}

#[test]
fn broader_tensor_forms_remain_explicitly_unsupported() {
    use eglur_sema::{CompileCapability, TypecheckCapability};
    for (source, capability) in [
        (
            "import std::tensor::ones\nfn make() -> Unit { let t = ones(2)\nyield () }\n",
            TypecheckCapability::AnalyzeTensorSource,
        ),
        (
            "import std::tensor::ones\nfn make() -> Unit { let t = ones[f32](dims = 2)\nyield () }\n",
            TypecheckCapability::AnalyzeTensorSource,
        ),
    ] {
        let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
        let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
        let catalog = TrustedCatalog::first_slice().unwrap();
        let analysis = session
            .resolve(&SupportProfile::tensor_constructors_v1(), Some(&catalog))
            .unwrap();
        assert!(
            analysis
                .trace()
                .missing()
                .any(|entry| entry.capability() == CompileCapability::Typecheck(capability)),
            "{source}"
        );
        assert!(session.seal(None).unwrap().checked_program().is_none());
    }
}

#[test]
fn aliases_normalize_before_closed_dtype_checks_and_nat_remains_exact() {
    reject(
        "import std::tensor::ones\nfn make() -> Unit { let t = ones[f32](-1)\nyield () }\n",
        "DOMAIN_INVALID_VALUE",
    );
    accept("import std::tensor::ones\nfn make() -> Unit { let t = ones[f32](-0)\nyield () }\n");
    accept(
        "import std::tensor::zeros\nimport std::tensor::Tensor\ntype Flag = Bool\ntype FlagAgain = Flag\nfn make() -> Tensor[Bool] = zeros[FlagAgain](1)\n",
    );
    let snapshot = check("import std::tensor::ones\ntype Text = Str\nfn make() -> Unit { let t = ones[Text](1)\nyield () }\n").into_diagnostics(None);
    assert_eq!(
        snapshot.diagnostics()[0].data().get("actual_type"),
        Some(&"Str".into())
    );
    reject(
        "import std::tensor::ones\nfn make(n: PositiveNat) -> Unit { let t = ones[f32](n)\nyield () }\n",
        "CALL_ARG_TYPE_MISMATCH",
    );
}
