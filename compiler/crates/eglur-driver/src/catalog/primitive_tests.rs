use super::*;
use crate::{
    CompileAttempt, CompileDisposition, CompilerSession, EvaluationLimits, ExecutionAttempt,
    FileInput, FileSetRequest, ProgramMode, SealedCompilation, SessionOptions, SupportProfile,
    SupportProfileId, SupportReason, execute,
};
use eglur_sema::{
    CanonicalTarget, CompileCapability, InvocationTarget, TrustedInherentOwnerHead,
    TypecheckCapability, UnavailableCompileInput,
};

fn analyze(
    files: &[(&str, &str)],
    catalog: &TrustedCatalog,
    profile: &SupportProfile,
) -> SealedCompilation {
    let request = FileSetRequest::new(
        files
            .iter()
            .map(|(path, source)| FileInput::new(*path, source.as_bytes().to_vec())),
        "main.eglur",
        (files.len() > 1).then_some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    session
        .resolve_for(ProgramMode::Library, profile, Some(catalog))
        .unwrap();
    session.seal(None).unwrap()
}

fn library(source: &str, catalog: &TrustedCatalog) -> SealedCompilation {
    analyze(
        &[("main.eglur", source)],
        catalog,
        &SupportProfile::semantic_core_v1(),
    )
}

fn require_identity(compilation: SealedCompilation, expected: &str) -> crate::CheckedProgram {
    assert!(
        !compilation.trace().has_unsupported(),
        "{:?}",
        compilation.trace()
    );
    assert!(
        compilation.diagnostics().diagnostics().is_empty(),
        "{:?}",
        compilation.diagnostics()
    );
    let program = compilation.into_checked_program().unwrap();
    let resolution = program.resolution();
    let bodies = resolution.body_types();
    let identities = bodies
        .invocations()
        .filter_map(|(_, call)| match call.target() {
            InvocationTarget::Callable(id) => Some(*id),
            _ => None,
        })
        .chain(bodies.callable_values().map(|(_, id)| id))
        .map(|id| {
            let instance = bodies.callable_instance(id).unwrap();
            resolution
                .signatures()
                .callable(instance.signature())
                .unwrap()
                .declaration()
                .clone()
        })
        .collect::<Vec<_>>();
    assert!(
        identities.contains(&CanonicalTarget::External(external_id(expected))),
        "{identities:?}"
    );
    program
}

#[test]
fn complete_primitive_signatures_typecheck_real_calls_with_exact_callable_identity() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (source, identity) in [
        (
            "fn use(v: Bool) -> Tri = v.to_tri()\n",
            "std::logic::Bool.to_tri",
        ),
        (
            "fn use(v: Bool, t: Tri) -> Tri = v.and(t)\n",
            "std::logic::Bool.and",
        ),
        (
            "fn use(v: Bool, t: Tri) -> Tri = Bool.or(v, t)\n",
            "std::logic::Bool.or",
        ),
        (
            "fn use(v: Str) -> Nat = v.len_bytes()\n",
            "std::str::Str.len_bytes",
        ),
        (
            "fn use(v: &Str) -> Bool = Str.is_empty(v)\n",
            "std::str::Str.is_empty",
        ),
        (
            "fn use(v: Str, s: &Str) -> Bool = v.contains(s)\n",
            "std::str::Str.contains",
        ),
        (
            "fn use(v: Str, s: &Str) -> Bool = v.starts_with(s)\n",
            "std::str::Str.starts_with",
        ),
        (
            "fn use(v: Str, s: &Str) -> Bool = v.ends_with(s)\n",
            "std::str::Str.ends_with",
        ),
        (
            "fn use(v: &Str) -> Str = ((Str.trim))(v)\n",
            "std::str::Str.trim",
        ),
        (
            "fn use(v: Str) -> Str = v.trim_start()\n",
            "std::str::Str.trim_start",
        ),
        (
            "fn use(v: Str) -> Str = v.trim_end()\n",
            "std::str::Str.trim_end",
        ),
        (
            "fn use(v: Str, a: &Str, b: &Str) -> Str = v.replace(a, b)\n",
            "std::str::Str.replace",
        ),
        (
            "fn use(v: Str) -> Seq[Str] = v.lines()\n",
            "std::str::Str.lines",
        ),
        (
            "fn use(v: Str, s: &Str) -> Seq[Str] = v.split(s)\n",
            "std::str::Str.split",
        ),
        (
            "fn use(v: Str, s: &Str) -> Seq[Str] = v.split_nonempty(s)\n",
            "std::str::Str.split_nonempty",
        ),
        (
            "fn use(v: Str, s: &Str) -> Opt[Str] = v.strip_prefix(s)\n",
            "std::str::Str.strip_prefix",
        ),
        (
            "fn use(v: Str, s: &Str) -> Opt[Str] = v.strip_suffix(s)\n",
            "std::str::Str.strip_suffix",
        ),
        (
            "fn use(v: Str) -> Str = v.to_lower_ascii()\n",
            "std::str::Str.to_lower_ascii",
        ),
        (
            "fn use(v: Str) -> Str = v.to_upper_ascii()\n",
            "std::str::Str.to_upper_ascii",
        ),
        (
            "fn use(v: i64) -> Res[Nat, DomainError] = Nat.from_i64(v)\n",
            "std::prelude::Nat.from_i64",
        ),
        (
            "fn use(v: Nat) -> i64 = v.to_i64()\n",
            "std::prelude::Nat.to_i64",
        ),
        (
            "fn use(v: i64) -> Res[PositiveNat, DomainError] = PositiveNat.from_i64(v)\n",
            "std::prelude::PositiveNat.from_i64",
        ),
        (
            "fn use(v: PositiveNat) -> Nat = PositiveNat.to_nat(v)\n",
            "std::prelude::PositiveNat.to_nat",
        ),
    ] {
        require_identity(library(source, &catalog), identity);
    }
}

#[test]
fn primitive_calls_and_fn_values_preserve_alias_import_and_reexport_identity() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for source in [
        "type Text = Str\nfn use(v: &Text) -> Str = Text.trim(v)\n",
        "import std::prelude::Str as Text\nfn use(v: &Text) -> Str = Text.trim(v)\n",
        "fn use(v: &Str) -> Str = std::prelude::Str.trim(v)\n",
        "fn use(v: &Str) -> Str { let f: Fn(&Str) -> Str = Str.trim\nyield f(v) }\n",
        "fn use(v: &Str) -> Str { let f = Str.trim\nyield f(v) }\n",
        "type Text = Str\nfn use(v: &Text) -> Str { let f: Fn(&Text) -> Str = ((Text.trim))\nyield f(v) }\n",
        "fn use(Str: &Str) -> Str = Str.trim()\n",
    ] {
        require_identity(library(source, &catalog), "std::str::Str.trim");
    }
    require_identity(
        analyze(
            &[
                (
                    "main.eglur",
                    "import bridge::Text\nfn use(v: &Text) -> Str = Text.trim(v)\n",
                ),
                ("bridge.eglur", "pub import std::prelude::Str as Text\n"),
            ],
            &catalog,
            &SupportProfile::semantic_core_v1(),
        ),
        "std::str::Str.trim",
    );
}

#[test]
fn dependency_type_metadata_does_not_invent_method_inventory() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for source in [
        "fn use(t: Tri) -> Tri = t\n",
        "fn use(t: Tri) -> Tri = match t { value => value }\n",
        "import std::seq::Seq as Items\nfn use(v: &Str) -> Items[Str] = Str.lines(v)\n",
        "type Seq = Bool\nfn use(v: &Str) -> std::seq::Seq[Str] = Str.lines(v)\n",
    ] {
        let compilation = library(source, &catalog);
        assert!(
            !compilation.trace().has_unsupported(),
            "{source}\n{:?}",
            compilation.trace()
        );
        assert!(
            compilation.diagnostics().diagnostics().is_empty(),
            "{source}\n{:?}",
            compilation.diagnostics()
        );
        assert!(compilation.checked_program().is_some());
    }
    for variant in ["True", "False", "Unknown"] {
        let source = format!("fn use(t: Tri) -> Tri = match t {{ {variant} => t }}\n");
        let attempt = CompileAttempt::finalize(library(&source, &catalog), None).unwrap();
        assert_eq!(attempt.disposition(), CompileDisposition::Rejected);
        assert_eq!(
            attempt
                .compiler_result()
                .unwrap()
                .primary_diagnostic()
                .unwrap()
                .code()
                .as_str(),
            "NAME_AMBIGUOUS",
            "{source}"
        );
    }
    for source in [
        "fn use(v: Seq[Str]) -> Unit = v.missing()\n",
        "fn use(v: Tri) -> Unit = v.missing()\n",
        "fn use(v: DomainError) -> Unit = v.missing()\n",
    ] {
        let compilation = library(source, &catalog);
        assert!(compilation.trace().has_unsupported(), "{source}");
        assert!(
            compilation.diagnostics().diagnostics().is_empty(),
            "{source}\n{:?}",
            compilation.diagnostics()
        );
    }
}

fn without_inventory(primitive: PrimitiveType) -> TrustedCatalog {
    let TrustedCatalog {
        names,
        signatures,
        inhabitance,
        effects,
        language_items,
        runtime_hooks,
    } = TrustedCatalog::first_slice().unwrap();
    let head = TrustedInherentOwnerHead::Primitive(primitive);
    let removed = signatures
        .inherent_owner(&head)
        .unwrap()
        .members
        .iter()
        .map(|m| m.callable.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let owners = signatures
        .inherent_owners()
        .filter(|owner| owner.head != head)
        .cloned()
        .collect::<Vec<_>>();
    let signatures = TrustedSignatureEnvironment::with_inherent_owners(
        signatures
            .declarations()
            .filter(|s| !removed.contains(s.id()))
            .cloned(),
        owners,
    )
    .unwrap();
    let effects = TrustedEffectEnvironment::new(
        effects
            .summaries()
            .filter(|s| !removed.contains(s.declaration()))
            .cloned(),
    )
    .unwrap();
    TrustedCatalog::new_with_language_items(
        names,
        signatures,
        inhabitance,
        effects,
        language_items,
        runtime_hooks,
    )
    .unwrap()
}

#[test]
fn absent_primitive_inventory_refuses_calls_values_and_trait_fallback_without_negative_evidence() {
    let catalog = without_inventory(PrimitiveType::Str);
    for source in [
        "fn use(v: &Str) -> Str = Str.trim(v)\n",
        "fn use(v: &Str) -> Str = ((Str.trim))(v)\n",
        "fn use(v: &Str) -> Str { let f: Fn(&Str) -> Str = Str.trim\nyield f(v) }\n",
        "fn use(v: Str) -> Str = v.to_str()\n",
        "type Text = Str\nfn use(v: &Text) -> Unit = Text.no_such_method(v)\n",
    ] {
        let compilation = library(source, &catalog);
        assert!(compilation.checked_program().is_none(), "{source}");
        assert!(
            compilation.diagnostics().diagnostics().is_empty(),
            "{source}\n{:?}",
            compilation.diagnostics()
        );
        assert!(compilation.trace().missing().next().is_none());
        let refusals = compilation
            .trace()
            .unsupported()
            .flat_map(|entry| entry.input_refusals())
            .collect::<Vec<_>>();
        assert!(
            refusals.iter().any(|r| r.input()
                == &UnavailableCompileInput::PrimitiveInherentInventory {
                    owner: PrimitiveType::Str
                }),
            "{source}\n{refusals:?}"
        );
        let attempt = CompileAttempt::finalize(compilation, None).unwrap();
        assert_eq!(attempt.disposition(), CompileDisposition::Unsupported);
        let json: serde_json::Value =
            serde_json::from_str(&attempt.development_attempt().unwrap().to_json()).unwrap();
        let inputs = json["capability_trace"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["capability"] == "typecheck.trusted_inherent_methods")
            .unwrap()["unavailable_inputs"]
            .as_array()
            .unwrap();
        assert_eq!(inputs[0]["kind"], "primitive_inherent_inventory");
        assert_eq!(inputs[0]["owner"], "std::prelude::Str");
    }
}

#[test]
fn inventory_presence_and_profile_selection_are_independent() {
    let core = SupportProfile::semantic_core_v1();
    let reduced = SupportProfile::new(
        core.schema(),
        SupportProfileId::new("without-primitive-inherent").unwrap(),
        core.compile_capabilities().filter(|c| {
            *c != CompileCapability::Typecheck(TypecheckCapability::ResolveTrustedInherentMethods)
        }),
        core.execution_capabilities(),
    )
    .unwrap();
    for (catalog, absent) in [
        (TrustedCatalog::first_slice().unwrap(), false),
        (without_inventory(PrimitiveType::Str), true),
    ] {
        for source in [
            "fn use(v: &Str) -> Str = Str.trim(v)\n",
            "fn use(v: Str) -> Str = v.trim()\n",
            "fn use(v: &Str) -> Str { let f: Fn(&Str) -> Str = Str.trim\nyield f(v) }\n",
        ] {
            let compilation = analyze(&[("main.eglur", source)], &catalog, &reduced);
            assert!(compilation.checked_program().is_none());
            assert!(compilation.diagnostics().diagnostics().is_empty());
            let entry = compilation
                .trace()
                .missing()
                .find(|entry| {
                    entry.capability()
                        == CompileCapability::Typecheck(
                            TypecheckCapability::ResolveTrustedInherentMethods,
                        )
                })
                .unwrap();
            assert_eq!(!entry.input_refusals().is_empty(), absent);
        }
    }
}

#[test]
fn completed_inventory_preserves_display_but_rejects_absent_and_inapplicable_members() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for primitive in PrimitiveType::ALL {
        let source = format!("fn use() -> Unit = {}.missing()\n", primitive.spelling());
        let attempt = CompileAttempt::finalize(library(&source, &catalog), None).unwrap();
        assert_eq!(attempt.disposition(), CompileDisposition::Rejected);
        assert_eq!(
            attempt
                .compiler_result()
                .unwrap()
                .primary_diagnostic()
                .unwrap()
                .code()
                .as_str(),
            "METHOD_NOT_FOUND",
            "{source}"
        );
    }
    for source in [
        "fn use(v: Str) -> Str = v.to_str()\n",
        "fn use(v: Bool) -> Str = v.to_str()\n",
        "fn use(v: i32) -> Str = v.to_str()\n",
    ] {
        let compilation = library(source, &catalog);
        assert!(compilation.diagnostics().diagnostics().is_empty());
        assert!(compilation.checked_program().is_some());
    }
    for (source, code) in [
        ("fn use() -> Unit = i32.missing()\n", "METHOD_NOT_FOUND"),
        ("fn use(v: Str) -> Unit = v.missing()\n", "METHOD_NOT_FOUND"),
        (
            "fn use(v: &Str) -> Str = Str.to_str(v)\n",
            "METHOD_NOT_FOUND",
        ),
        (
            "fn use(v: &Bool) -> Str = Str.trim(v)\n",
            "CALL_ARG_TYPE_MISMATCH",
        ),
        (
            "fn use(v: Bool) -> Str = Str.trim(v)\n",
            "CALL_ARG_TYPE_MISMATCH",
        ),
        (
            "fn use(v: Bool) -> Bool = v.and(true)\n",
            "METHOD_NOT_FOUND",
        ),
        (
            "fn use(v: Bool) -> Bool = Bool.and(v, true)\n",
            "CALL_ARG_TYPE_MISMATCH",
        ),
        (
            "fn use(v: Bool) -> Bool = v.and!(true)\n",
            "METHOD_NOT_FOUND",
        ),
        (
            "trait Fallback { fn trim(self: &Self, n: i64) -> Str }\nimpl Fallback for Str { fn trim(self: &Self, n: i64) -> Str = \"fallback\" }\nfn use(v: Str) -> Str = v.trim(1)\n",
            "METHOD_NOT_FOUND",
        ),
    ] {
        let attempt = CompileAttempt::finalize(library(source, &catalog), None).unwrap();
        assert_eq!(
            attempt.disposition(),
            CompileDisposition::Rejected,
            "{source}\n{}",
            attempt.render_human()
        );
        assert_eq!(
            attempt
                .compiler_result()
                .unwrap()
                .primary_diagnostic()
                .unwrap()
                .code()
                .as_str(),
            code,
            "{source}\n{}",
            attempt.render_human()
        );
    }
}

#[test]
fn available_primitive_metadata_without_hooks_refuses_before_execution() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let profile = SupportProfile::semantic_core_v1();
    for body in [
        "let value: Str = \" input \"\n_ = value.trim()",
        "let value: Str = \" input \"\n_ = ((Str.trim))(&value)",
        "let f: Fn(&Str) -> Str = Str.trim\n_ = f",
    ] {
        let source = format!(
            "import std::test::emit!\nfn main!() -> Unit {{ emit!(\"must not execute\")\n{body}\nyield () }}\n"
        );
        let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
        let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
        session
            .resolve_for(ProgramMode::Executable, &profile, Some(&catalog))
            .unwrap();
        let program = require_identity(session.seal(None).unwrap(), "std::str::Str.trim");
        assert_eq!(
            execute(&program, &profile, &catalog, EvaluationLimits::default()),
            ExecutionAttempt::Unsupported(SupportReason::MissingRuntimeHook(external_id(
                "std::str::Str.trim"
            ))),
            "{source}"
        );
    }
}
