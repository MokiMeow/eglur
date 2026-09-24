use eglur_driver::{
    CompilerSession, FileInput, FileSetRequest, SessionOptions, SupportProfile, SupportProfileId,
    SupportSchemaVersion, TrustedCatalog,
};
use eglur_hir::ExprKind;
use eglur_sema::{
    CanonicalTarget, CompileCapability, FactState, ModuleId, ResolutionKey, ResolvedName,
    TypecheckCapability,
};

fn inspect(
    files: &[(&str, &str)],
    expected: &str,
) -> (
    Option<ResolvedName>,
    ModuleId,
    Vec<String>,
    Vec<CompileCapability>,
) {
    let request = FileSetRequest::new(
        files
            .iter()
            .map(|(path, text)| FileInput::new(*path, text.as_bytes().to_vec())),
        "main.eglur",
        (files.len() > 1).then_some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let source = session
        .sources()
        .iter()
        .find(|source| source.path() == "main.eglur")
        .unwrap();
    let file = source.file();
    let expression = source
        .hir()
        .unwrap()
        .bodies()
        .flat_map(|(_, body)| body.expressions())
        .find_map(|(id, expression)| match expression.kind() {
            ExprKind::Member { member, .. } if member.text() == Some(expected) => Some(id),
            _ => None,
        })
        .unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().unwrap();
    let module = resolution.declarations().module_for_file(file).unwrap();
    let fact = match resolution
        .names()
        .fact(ResolutionKey::Expression { module, expression })
    {
        FactState::Known(found) => Some(found.clone()),
        FactState::Poisoned(_) | FactState::Unperformed => None,
        other => panic!("unexpected qualified member identity: {other:?}"),
    };
    let missing = analysis
        .trace()
        .missing()
        .map(|entry| entry.capability())
        .collect();
    let diagnostics = session
        .into_diagnostics(None)
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str().to_owned())
        .collect();
    (fact, module, diagnostics, missing)
}

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
fn trait_qualified_member_keeps_a_type_owner_without_a_runtime_trait_value() {
    let (fact, module, diagnostics, missing) = inspect(
        &[(
            "main.eglur",
            "trait Rank { fn rank(self: &Self) -> i64 }\nfn use_rank(f: Fn(&Bool) -> i64) -> Unit = ()\nfn source() -> Unit = use_rank(Rank.rank)\n",
        )],
        "rank",
    );
    let Some(ResolvedName::TypeMemberOwner(CanonicalTarget::Source(owner))) = fact else {
        panic!("trait member did not retain its type owner: {fact:?}");
    };
    assert_eq!(owner.module(), module);
    assert!(!diagnostics.iter().any(|code| code == "NAME_NOT_FOUND"));
    assert!(
        diagnostics
            .iter()
            .any(|code| code == "TRAIT_BOUND_NOT_SATISFIED")
    );
    assert!(!missing.contains(&CompileCapability::Typecheck(
        TypecheckCapability::CheckClosures
    )));
}

#[test]
fn imported_trait_qualified_member_keeps_the_original_owner() {
    let (fact, module, diagnostics, missing) = inspect(
        &[
            (
                "main.eglur",
                "import dep::Rank\nfn use_rank(f: Fn(&Bool) -> i64) -> Unit = ()\nfn source() -> Unit = use_rank(Rank.rank)\n",
            ),
            (
                "dep.eglur",
                "pub trait Rank { fn rank(self: &Self) -> i64 }\n",
            ),
        ],
        "rank",
    );
    let Some(ResolvedName::TypeMemberOwner(CanonicalTarget::Source(owner))) = fact else {
        panic!("imported trait member lost its owner: {fact:?}");
    };
    assert_ne!(owner.module(), module);
    assert!(!diagnostics.iter().any(|code| code == "NAME_NOT_FOUND"));
    assert!(
        diagnostics
            .iter()
            .any(|code| code == "TRAIT_BOUND_NOT_SATISFIED")
    );
    assert!(!missing.contains(&CompileCapability::Typecheck(
        TypecheckCapability::CheckClosures
    )));
}

#[test]
fn module_qualified_trait_member_keeps_the_original_owner() {
    let (fact, module, diagnostics, missing) = inspect(
        &[
            (
                "main.eglur",
                "fn use_rank(f: Fn(&Bool) -> i64) -> Unit = ()\nfn source() -> Unit = use_rank(dep::Rank.rank)\n",
            ),
            (
                "dep.eglur",
                "pub trait Rank { fn rank(self: &Self) -> i64 }\n",
            ),
        ],
        "rank",
    );
    let Some(ResolvedName::TypeMemberOwner(CanonicalTarget::Source(owner))) = fact else {
        panic!("module-qualified trait member lost its owner: {fact:?}");
    };
    assert_ne!(owner.module(), module);
    assert!(!diagnostics.iter().any(|code| code == "NAME_NOT_FOUND"));
    assert!(
        diagnostics
            .iter()
            .any(|code| code == "TRAIT_BOUND_NOT_SATISFIED")
    );
    assert!(!missing.contains(&CompileCapability::Typecheck(
        TypecheckCapability::CheckClosures
    )));
}

#[test]
fn value_receiver_wins_over_a_same_spelled_trait() {
    let (fact, _, diagnostics, missing) = inspect(
        &[(
            "main.eglur",
            "trait Rank { fn rank(self: &Self) -> i64 }\nstruct Boxed { field rank: i64 }\nfn source(Rank: Boxed) -> i64 = Rank.rank\n",
        )],
        "rank",
    );
    assert!(
        fact.is_none(),
        "ordinary value member got a static owner: {fact:?}"
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(!missing.contains(&CompileCapability::Typecheck(
        TypecheckCapability::CheckClosures
    )));
}

#[test]
fn qualified_unit_variant_remains_a_value_receiver() {
    let (fact, _, diagnostics, missing) = inspect(
        &[(
            "main.eglur",
            "enum Choice { Unit }\ntrait Rank { fn rank(self: &Self) -> i64 }\nimpl Rank for Choice { fn rank(self: &Self) -> i64 = 1 }\nfn source() -> i64 = Choice::Unit.rank()\n",
        )],
        "rank",
    );
    assert!(fact.is_none(), "unit variant got a static owner: {fact:?}");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(!missing.contains(&CompileCapability::Typecheck(
        TypecheckCapability::CheckClosures
    )));
}

#[test]
fn reexported_trait_member_keeps_the_original_type_owner() {
    let (fact, module, diagnostics, missing) = inspect(
        &[
            (
                "main.eglur",
                "import bridge::PublicRank\nfn use_rank(f: Fn(&Bool) -> i64) -> Unit = ()\nfn source() -> Unit = use_rank(PublicRank.rank)\n",
            ),
            ("bridge.eglur", "pub import dep::Rank as PublicRank\n"),
            (
                "dep.eglur",
                "pub trait Rank { fn rank(self: &Self) -> i64 }\n",
            ),
        ],
        "rank",
    );
    let Some(ResolvedName::TypeMemberOwner(CanonicalTarget::Source(owner))) = fact else {
        panic!("reexported trait member lost its owner: {fact:?}");
    };
    assert_ne!(owner.module(), module);
    assert!(!diagnostics.iter().any(|code| code == "NAME_NOT_FOUND"));
    assert!(
        diagnostics
            .iter()
            .any(|code| code == "TRAIT_BOUND_NOT_SATISFIED")
    );
    assert!(!missing.contains(&CompileCapability::Typecheck(
        TypecheckCapability::CheckClosures
    )));
}

#[test]
fn grouped_trait_qualifier_keeps_its_type_identity() {
    let (fact, _, diagnostics, missing) = inspect(
        &[(
            "main.eglur",
            "trait Rank { fn rank(self: &Self) -> i64 }\nfn use_rank(f: Fn(&Bool) -> i64) -> Unit = ()\nfn source() -> Unit = use_rank((Rank).rank)\n",
        )],
        "rank",
    );
    assert!(matches!(fact, Some(ResolvedName::TypeMemberOwner(_))));
    assert!(!diagnostics.iter().any(|code| code == "NAME_NOT_FOUND"));
    assert!(
        diagnostics
            .iter()
            .any(|code| code == "TRAIT_BOUND_NOT_SATISFIED")
    );
    assert!(!missing.contains(&CompileCapability::Typecheck(
        TypecheckCapability::CheckClosures
    )));
}

#[test]
fn absent_member_receiver_keeps_name_not_found() {
    assert_eq!(
        diagnostic_codes("fn source() -> i64 = Missing.rank\n"),
        ["NAME_NOT_FOUND"]
    );
}

#[test]
fn private_qualified_trait_owner_keeps_visibility_failure() {
    let (_, _, diagnostics, _) = inspect(
        &[
            (
                "main.eglur",
                "fn use_rank(f: Fn(&Bool) -> i64) -> Unit = ()\nfn source() -> Unit = use_rank(dep::Rank.rank)\n",
            ),
            ("dep.eglur", "trait Rank { fn rank(self: &Self) -> i64 }\n"),
        ],
        "rank",
    );
    assert!(
        diagnostics
            .iter()
            .any(|code| code == "VISIBILITY_VIOLATION"),
        "{diagnostics:?}"
    );
    assert!(!diagnostics.iter().any(|code| code == "NAME_NOT_FOUND"));
}

#[test]
fn trusted_trait_member_keeps_external_owner() {
    let (fact, _, diagnostics, missing) = inspect(
        &[(
            "main.eglur",
            "fn use_display(f: Fn(&Bool) -> Str) -> Unit = ()\nfn source() -> Unit = use_display(Display.to_str)\n",
        )],
        "to_str",
    );
    assert!(matches!(
        fact,
        Some(ResolvedName::TypeMemberOwner(CanonicalTarget::External(_)))
    ));
    assert!(!diagnostics.iter().any(|code| code == "NAME_NOT_FOUND"));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(!missing.contains(&CompileCapability::Typecheck(
        TypecheckCapability::CheckClosures
    )));
}

#[test]
fn qualified_trait_method_without_impl_reports_unsatisfied_bound() {
    let prefix = "trait Rank { fn rank(self: &Self) -> i64 }\nstruct Count {}\n";
    for body in [
        "fn source(value: &Count) -> i64 = Rank.rank(value)\n",
        "fn source() -> Unit { let f: Fn(&Count) -> i64 = Rank.rank\nyield () }\n",
    ] {
        let codes = diagnostic_codes(&format!("{prefix}{body}"));
        assert_eq!(codes, ["TRAIT_BOUND_NOT_SATISFIED"], "{body}");
    }
}

#[test]
fn generic_qualified_trait_method_without_impl_reports_unsatisfied_bound() {
    let prefix = "trait Link[A] { fn forward(self: &Self, value: A) -> A }\nstruct Count {}\n";
    for body in [
        "fn source(value: &Count) -> i64 = Link.forward(value, 1)\n",
        "fn source() -> Unit { let f: Fn(&Count, i64) -> i64 = Link.forward\nyield () }\n",
    ] {
        let codes = diagnostic_codes(&format!("{prefix}{body}"));
        assert_eq!(codes, ["TRAIT_BOUND_NOT_SATISFIED"], "{body}");
    }
}

#[test]
fn qualified_trait_call_shape_precedes_argument_typing() {
    let codes = diagnostic_codes(concat!(
        "trait Link { fn forward(self: &Self, extra: i64) -> i64 }\n",
        "impl Link for Bool { fn forward(self: &Self, extra: i64) -> i64 = extra }\n",
        "fn bad(value: &Bool) -> i64 = Link.forward(extra=1, \"not checked\")\n",
    ));
    assert_eq!(codes, ["CALL_POSITIONAL_AFTER_NAMED"]);
}

#[test]
fn qualified_trait_value_requires_an_expected_receiver_type() {
    let codes = diagnostic_codes(concat!(
        "trait Rank { fn rank(self: &Self) -> i64 }\n",
        "impl Rank for Bool { fn rank(self: &Self) -> i64 = 1 }\n",
        "fn source() -> Unit { let f = Rank.rank\nyield () }\n",
    ));
    assert_eq!(codes, ["TYPE_ARG_INFERENCE_FAILED"]);
}

#[test]
fn qualified_trait_value_requires_method_resolution_capability() {
    let source = concat!(
        "trait Rank { fn rank(self: &Self) -> i64 }\n",
        "impl Rank for Bool { fn rank(self: &Self) -> i64 = 1 }\n",
        "fn source() -> Unit { let f: Fn(&Bool) -> i64 = Rank.rank\nyield () }\n",
    );
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let profile = SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("without-methods-v1").unwrap(),
        SupportProfile::semantic_core_v1()
            .compile_capabilities()
            .filter(|capability| {
                *capability != CompileCapability::Typecheck(TypecheckCapability::ResolveMethods)
            }),
    )
    .unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(analysis.trace().missing().any(|demand| {
        demand.capability() == CompileCapability::Typecheck(TypecheckCapability::ResolveMethods)
    }));
}

#[test]
fn concrete_owner_method_keeps_its_type_identity() {
    let (fact, module, diagnostics, missing) = inspect(
        &[(
            "main.eglur",
            "struct Count { fn read(self: &Self) -> i64 = 7 }\nfn use_read(f: Fn(&Count) -> i64) -> Unit = ()\nfn source() -> Unit = use_read(Count.read)\n",
        )],
        "read",
    );
    assert!(matches!(
        fact,
        Some(ResolvedName::TypeMemberOwner(CanonicalTarget::Source(owner))) if owner.module() == module
    ));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn transparent_alias_uses_the_underlying_inherent_method_for_calls_and_values() {
    let (fact, _, diagnostics, missing) = inspect(
        &[(
            "main.eglur",
            "struct Count { fn read(self: &Self) -> i64 = 7 }\n\
             type Alias = Count\n\
             fn accept(f: Fn(&Count) -> i64) -> Unit = ()\n\
             fn source(value: &Alias) -> i64 = Alias.read(value)\n\
             fn use_value() -> Unit = accept(Alias.read)\n",
        )],
        "read",
    );
    assert!(matches!(fact, Some(ResolvedName::TypeMemberOwner(_))));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn fixed_generic_alias_rejects_another_receiver_specialization() {
    let prefix = "struct Boxed[T] { field value: T\nfn read(self: &Self) -> T = self.value }\n\
                  type IntBox = Boxed[i64]\n";
    let direct = format!("{prefix}fn source(value: &Boxed[Str]) -> Str = IntBox.read(value)\n");
    assert_eq!(diagnostic_codes(&direct), ["CALL_ARG_TYPE_MISMATCH"]);

    let value = format!(
        "{prefix}fn accept(f: Fn(&Boxed[Str]) -> Str) -> Unit = ()\n\
         fn source() -> Unit = accept(IntBox.read)\n"
    );
    assert_eq!(diagnostic_codes(&value), ["CALL_ARG_TYPE_MISMATCH"]);
}

#[test]
fn unapplied_generic_alias_is_not_a_method_owner() {
    assert_eq!(
        diagnostic_codes(concat!(
            "struct Boxed[T] { field value: T\nfn read(self: &Self) -> T = self.value }\n",
            "type Alias[T] = Boxed[T]\n",
            "fn source(value: &Boxed[i64]) -> i64 = Alias.read(value)\n",
        )),
        ["TYPE_ARG_ARITY_MISMATCH"]
    );
}

#[test]
fn primitive_alias_has_no_concrete_method_namespace() {
    assert_eq!(
        diagnostic_codes("type Alias = i64\nfn source(value: Alias) -> i64 = Alias.read(value)\n"),
        ["METHOD_NOT_FOUND"]
    );
}

#[test]
fn alias_qualified_method_respects_private_family_visibility() {
    let (fact, _, diagnostics, _) = inspect(
        &[
            (
                "main.eglur",
                "import dep::Alias\nfn source(value: &Alias) -> i64 = Alias.read(value)\n",
            ),
            (
                "dep.eglur",
                "pub struct Count { fn read(self: &Self) -> i64 = 7 }\npub type Alias = Count\n",
            ),
        ],
        "read",
    );
    assert!(matches!(fact, Some(ResolvedName::TypeMemberOwner(_))));
    assert_eq!(diagnostics, ["VISIBILITY_VIOLATION"]);
}

#[test]
fn value_receiver_still_wins_over_a_same_spelled_type_alias() {
    let (fact, _, diagnostics, missing) = inspect(
        &[(
            "main.eglur",
            "struct Count { fn read(self: &Self) -> i64 = 7 }\n\
             type Alias = Count\n\
             fn source(Alias: Count) -> i64 = Alias.read()\n",
        )],
        "read",
    );
    assert!(fact.is_none(), "local value became a type owner: {fact:?}");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn transparent_alias_does_not_inherit_trait_impl_methods() {
    assert_eq!(
        diagnostic_codes(concat!(
            "trait Rank { fn rank(self: &Self) -> i64 }\n",
            "struct Count {}\n",
            "type Alias = Count\n",
            "impl Rank for Count { fn rank(self: &Self) -> i64 = 1 }\n",
            "fn source(value: &Alias) -> i64 = Alias.rank(value)\n",
        )),
        ["METHOD_NOT_FOUND"]
    );
}

#[test]
fn concrete_owner_does_not_inherit_trait_impl_methods() {
    let prefix = "trait Rank { fn rank(self: &Self) -> i64 }\nstruct Count {}\nimpl Rank for Count { fn rank(self: &Self) -> i64 = 1 }\n";
    for expression in ["Count.rank(&value)", "Count.rank"] {
        let source = format!("{prefix}fn source(value: &Count) -> i64 = {expression}\n");
        assert_eq!(
            diagnostic_codes(&source),
            ["METHOD_NOT_FOUND"],
            "{expression}"
        );
    }
}

#[test]
fn inaccessible_concrete_owner_method_precedes_applicability() {
    let (fact, _, diagnostics, _) = inspect(
        &[
            (
                "main.eglur",
                "import dep::Count\nfn source(value: &Count) -> i64 = Count.read(value)\n",
            ),
            (
                "dep.eglur",
                "pub struct Count { fn read(self: &Self) -> i64 = 7 }\n",
            ),
        ],
        "read",
    );
    assert!(matches!(fact, Some(ResolvedName::TypeMemberOwner(_))));
    assert_eq!(diagnostics, ["VISIBILITY_VIOLATION"]);
}

#[test]
fn concrete_owner_missing_member_is_not_a_missing_value_name() {
    assert_eq!(
        diagnostic_codes("struct Count {}\nfn source(value: &Count) -> i64 = Count.read(value)\n"),
        ["METHOD_NOT_FOUND"]
    );
}

#[test]
fn local_value_receiver_wins_over_a_same_spelled_concrete_owner() {
    let (fact, _, diagnostics, missing) = inspect(
        &[(
            "main.eglur",
            "struct Count { fn read(self: &Self) -> i64 = 7 }\nfn source(Count: Count) -> i64 = Count.read()\n",
        )],
        "read",
    );
    assert!(fact.is_none(), "local value became a type owner: {fact:?}");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn concrete_owner_call_requires_the_owner_receiver_type() {
    assert_eq!(
        diagnostic_codes(concat!(
            "struct Count { fn read(self: &Self) -> i64 = 7 }\n",
            "struct Other {}\n",
            "fn source(value: &Other) -> i64 = Count.read(value)\n",
        )),
        ["CALL_ARG_TYPE_MISMATCH"]
    );
}

#[test]
fn single_concrete_method_value_uses_the_enclosing_type_mismatch() {
    assert_eq!(
        diagnostic_codes(concat!(
            "struct Count { fn read(self: &Self) -> i64 = 7 }\n",
            "fn accept(f: Fn(&Count) -> Str) -> Unit = ()\n",
            "fn source() -> Unit = accept(Count.read)\n",
        )),
        ["CALL_ARG_TYPE_MISMATCH"]
    );
}
