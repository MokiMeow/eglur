use eglur_diagnostics::{DiagnosticDataValue, DiagnosticSnapshot, render_plain};
use eglur_driver::{
    CompilerSession, FileInput, FileSetRequest, Phase, SessionOptions, SupportProfile,
    SupportProfileId, SupportSchemaVersion, TrustedCatalog,
};
use eglur_sema::{
    CanonicalTarget, CompileCapability, DeclarationKind, EffectCategory, EffectSummary, FactState,
    PhaseProduct, ResolveCapability, StateScope, TypecheckCapability,
};

fn effect_profile() -> SupportProfile {
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("effect-analysis-v1").unwrap(),
        [
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
            CompileCapability::Typecheck(TypecheckCapability::ResolveOverloads),
            CompileCapability::Typecheck(TypecheckCapability::AnalyzeInhabitance),
            CompileCapability::Typecheck(TypecheckCapability::CheckFailureFlow),
            CompileCapability::Typecheck(
                TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
            ),
            CompileCapability::Typecheck(TypecheckCapability::CheckBranching),
            CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEvidence),
            CompileCapability::Typecheck(TypecheckCapability::ComputeEffectSummaries),
        ],
    )
    .unwrap()
}

fn file(path: &str, source: &str) -> FileInput {
    FileInput::new(path, source.as_bytes().to_vec())
}

fn session(files: impl IntoIterator<Item = FileInput>, entry: &str) -> CompilerSession {
    CompilerSession::build(
        FileSetRequest::new(files, entry, Some(".")).unwrap(),
        SessionOptions::new(true),
    )
    .unwrap()
}

fn single(source: &str) -> CompilerSession {
    CompilerSession::build(
        FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap(),
        SessionOptions::new(true),
    )
    .unwrap()
}

fn source_callable(resolution: &eglur_driver::SemanticResolution, name: &str) -> eglur_sema::DefId {
    resolution
        .declarations()
        .modules()
        .flat_map(eglur_sema::ModuleDeclarations::declarations)
        .find(|declaration| {
            declaration.kind() == DeclarationKind::Function
                && declaration
                    .name()
                    .is_some_and(|found| found.spelling() == name)
        })
        .map(eglur_sema::Declaration::id)
        .unwrap_or_else(|| panic!("missing source callable {name}"))
}

fn effect_facts(resolution: &eglur_driver::SemanticResolution) -> &eglur_sema::EffectFacts {
    let PhaseProduct::Performed(facts) = resolution.effect_facts() else {
        panic!("effect analysis must be performed")
    };
    facts
}

fn known_summary(
    facts: &eglur_sema::EffectFacts,
    declaration: eglur_sema::DefId,
) -> &eglur_sema::KnownEffectSummary {
    let state = facts.source_summary(declaration);
    let FactState::Known(EffectSummary::Known(summary)) = state else {
        panic!("source callable must have a known effect summary, found {state:?}")
    };
    summary
}

#[test]
fn imported_localizable_state_is_discharged_for_a_non_escaping_local() {
    let mut session = session(
        [
            file(
                "dep.eglur",
                concat!(
                    "pub fn bump!(value: &mut i64) -> Unit {\n",
                    "  *value = 2\n",
                    "  yield ()\n",
                    "}\n",
                ),
            ),
            file(
                "main.eglur",
                concat!(
                    "import dep::bump!\n",
                    "fn main() -> i64 {\n",
                    "  let mut value = 1\n",
                    "  bump!(&mut value)\n",
                    "  yield value\n",
                    "}\n",
                ),
            ),
        ],
        "main.eglur",
    );
    let catalog = TrustedCatalog::first_slice().unwrap();

    {
        let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
        let resolution = analysis.resolution().unwrap();
        let facts = effect_facts(resolution);
        let bump_id = source_callable(resolution, "bump!");
        let main_id = source_callable(resolution, "main");
        let bump = known_summary(facts, bump_id);
        let main = known_summary(facts, main_id);

        assert!(analysis.trace().satisfied().any(|entry| {
            entry.capability()
                == CompileCapability::Typecheck(TypecheckCapability::ComputeEffectSummaries)
        }));
        assert_eq!(bump.categories(), [EffectCategory::State]);
        assert_eq!(bump.state_scope(), StateScope::Localizable);
        assert!(main.is_pure());
    }
    let observation = session
        .observations()
        .iter()
        .find(|observation| observation.phase() == Phase::EffectAnalysis)
        .expect("selected effect phase is observed");
    assert_eq!(observation.input_count(), 2);
    assert_eq!(observation.output_count(), 22);
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn rebinding_a_local_reference_does_not_mutate_its_referent() {
    let mut session = single(concat!(
        "fn rebind!(target: &mut i64) -> Unit {\n",
        "  let mut local = 0\n",
        "  let mut reference = target\n",
        "  reference = &mut local\n",
        "  yield ()\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let summary = known_summary(
        effect_facts(resolution),
        source_callable(resolution, "rebind!"),
    );

    assert!(summary.is_pure());
    assert!(session.into_diagnostics(None).diagnostics().is_empty());
}

#[test]
fn escaped_local_cannot_reenter_the_local_state_exception() {
    let mut session = single(concat!(
        "fn bump!(value: &mut i64) -> Unit {\n",
        "  *value = 2\n",
        "  yield ()\n",
        "}\n",
        "fn main() -> i64 {\n",
        "  let mut value = 1\n",
        "  return value\n",
        "  bump!(&mut value)\n",
        "  yield value\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&effect_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);

    assert_eq!(
        snapshot.diagnostics().len(),
        1,
        "unexpected diagnostics: {:?}",
        snapshot
            .diagnostics()
            .iter()
            .map(|diagnostic| (diagnostic.code().as_str(), diagnostic.message()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        snapshot.diagnostics()[0].code().as_str(),
        "EFFECT_VIOLATION"
    );
}

#[test]
fn imported_transitive_io_is_rejected_in_a_non_bang_caller() {
    let mut session = session(
        [
            file(
                "dep.eglur",
                concat!(
                    "import std::io::print!\n",
                    "import std::io::TextError\n",
                    "pub fn write!() -> Res[Unit, TextError] = print!(\"hello\")\n",
                ),
            ),
            file(
                "main.eglur",
                concat!(
                    "import dep::write!\n",
                    "import std::io::TextError\n",
                    "fn main() -> Res[Unit, TextError] = write!()\n",
                ),
            ),
        ],
        "main.eglur",
    );
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&effect_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(
        diagnostics.len(),
        1,
        "unexpected diagnostics: {:?}",
        diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code().as_str(), diagnostic.message()))
            .collect::<Vec<_>>()
    );
    assert_eq!(diagnostics[0].code().as_str(), "EFFECT_VIOLATION");
    assert_eq!(diagnostics[0].phase().as_str(), "effect");
    assert_eq!(
        diagnostics[0].data().get("caller"),
        Some(&DiagnosticDataValue::from("main"))
    );
    assert_eq!(
        diagnostics[0].data().get("callee"),
        Some(&DiagnosticDataValue::from("write!"))
    );
    assert_eq!(diagnostics[0].related().len(), 1);
    assert_eq!(
        diagnostics[0].related()[0].label(),
        "effectful callee declared here"
    );
    assert_eq!(
        render_plain(&snapshot),
        concat!(
            "error[EFFECT_VIOLATION] non-bang function `main` cannot call `write!`\n",
            " --> main.eglur:3:36\n",
            " = help: mark caller as `main!` or remove the effectful operation\n",
            " = callee: write!\n",
            " = caller: main\n",
            " = related: effectful callee declared here\n",
            "   --> dep.eglur:3:1-4:1",
        )
    );
}

#[test]
fn bounded_generic_bang_trait_call_has_unknown_summary() {
    let mut session = single(concat!(
        "trait Runnable { fn run!(self: &mut Self) -> Unit }\n",
        "struct Job {}\n",
        "impl Runnable for Job { fn run!(self: &mut Self) -> Unit = () }\n",
        "fn invoke![T: Runnable](value: &mut T) -> Unit = value.run!()\n",
        "fn caller() -> Unit {\n",
        "  let mut job = Job {}\n",
        "  invoke!::[Job](&mut job)\n",
        "  yield ()\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let invoke = source_callable(resolution, "invoke!");

    assert!(matches!(
        effect_facts(resolution).source_summary(invoke),
        FactState::Known(EffectSummary::Unknown)
    ));
    let snapshot = session.into_diagnostics(None);
    assert_eq!(snapshot.diagnostics().len(), 1);
    assert_eq!(
        snapshot.diagnostics()[0].code().as_str(),
        "EFFECT_VIOLATION"
    );
    assert_eq!(
        snapshot.diagnostics()[0].data().get("callee"),
        Some(&DiagnosticDataValue::from("invoke!"))
    );
}

#[test]
fn dead_if_branch_cannot_hide_an_effectful_call() {
    let mut session = single(concat!(
        "import std::io::print!\n",
        "fn f() -> i64 {\n",
        "  if false {\n",
        "    print!(\"dead\")\n",
        "  } else {\n",
        "    _ = 0\n",
        "  }\n",
        "  yield 0\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&effect_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);

    assert_eq!(
        snapshot.diagnostics().len(),
        1,
        "unexpected diagnostics: {:?}",
        snapshot
            .diagnostics()
            .iter()
            .map(|diagnostic| (diagnostic.code().as_str(), diagnostic.message()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        snapshot.diagnostics()[0].code().as_str(),
        "EFFECT_VIOLATION"
    );
    assert!(snapshot.diagnostics()[0].message().contains("print!"));
}

#[test]
fn pure_mutual_recursion_converges_to_one_pure_scc() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for source in [
        concat!(
            "fn left(value: i64) -> i64 = right(value)\n",
            "fn right(value: i64) -> i64 = left(value)\n",
        ),
        concat!(
            "fn right(value: i64) -> i64 = left(value)\n",
            "fn left(value: i64) -> i64 = right(value)\n",
        ),
    ] {
        let mut session = single(source);
        let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
        let resolution = analysis.resolution().unwrap();
        let facts = effect_facts(resolution);
        let left = source_callable(resolution, "left");
        let right = source_callable(resolution, "right");

        assert!(known_summary(facts, left).is_pure());
        assert!(known_summary(facts, right).is_pure());
        assert!(
            facts
                .sccs()
                .iter()
                .any(|scc| { scc.members() == [left, right] || scc.members() == [right, left] })
        );
    }
}

#[test]
fn recursive_scc_propagates_io_to_every_member() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for declarations in [
        concat!(
            "fn left!() -> Res[Unit, TextError] = right!()\n",
            "fn right!() -> Res[Unit, TextError] = if true {\n",
            "  yield print!(\"hello\")\n",
            "} else {\n",
            "  yield left!()\n",
            "}\n",
        ),
        concat!(
            "fn right!() -> Res[Unit, TextError] = if true {\n",
            "  yield print!(\"hello\")\n",
            "} else {\n",
            "  yield left!()\n",
            "}\n",
            "fn left!() -> Res[Unit, TextError] = right!()\n",
        ),
    ] {
        let source = format!("import std::io::print!\nimport std::io::TextError\n{declarations}");
        let mut session = single(&source);
        let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
        let resolution = analysis.resolution().unwrap();
        let facts = effect_facts(resolution);

        for name in ["left!", "right!"] {
            let summary = known_summary(facts, source_callable(resolution, name));
            assert_eq!(summary.categories(), [EffectCategory::Io]);
            assert_eq!(summary.state_scope(), StateScope::None);
        }
        assert!(facts.summaries().any(|(target, state)| {
            matches!(
                (target, state),
                (
                    CanonicalTarget::External(_),
                    FactState::Known(EffectSummary::Known(_))
                )
            )
        }));
    }
}

#[test]
fn poisoned_body_does_not_emit_a_derivative_effect_violation() {
    let mut session = single("fn main() -> Unit = missing()\n");
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let main = source_callable(resolution, "main");

    assert!(matches!(
        effect_facts(resolution).source_summary(main),
        FactState::Poisoned(_)
    ));
    let snapshot = session.into_diagnostics(None);
    assert_eq!(snapshot.diagnostics().len(), 1);
    assert_eq!(snapshot.diagnostics()[0].code().as_str(), "NAME_NOT_FOUND");
}

#[test]
fn poisoned_trait_dispatch_poison_propagates_to_the_effect_fact() {
    let mut session = single(concat!(
        "trait Mark {}\n",
        "struct Plain {}\n",
        "fn bounded[T: Mark](value: T) -> T = value\n",
        "fn invalid(value: Plain) -> Plain = bounded(value)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let invalid = source_callable(resolution, "invalid");

    assert!(matches!(
        effect_facts(resolution).source_summary(invalid),
        FactState::Poisoned(_)
    ));
    let snapshot = session.into_diagnostics(None);
    assert_eq!(snapshot.diagnostics().len(), 1);
    assert_eq!(
        snapshot.diagnostics()[0].code().as_str(),
        "TRAIT_BOUND_NOT_SATISFIED"
    );
}

#[test]
fn unsupported_body_operation_keeps_the_effect_fact_unperformed() {
    let mut session = single("fn add(left: f64, right: f64) -> f64 = left + right\n");
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let add = source_callable(resolution, "add");

    assert!(matches!(
        effect_facts(resolution).source_summary(add),
        FactState::Unperformed
    ));
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckOperators)
    }));
    assert!(session.into_diagnostics(None).diagnostics().is_empty());
}
