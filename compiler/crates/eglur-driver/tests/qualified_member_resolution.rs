use eglur_driver::{
    CompilerSession, FileInput, FileSetRequest, SessionOptions, SupportProfile, TrustedCatalog,
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
    assert!(missing.contains(&CompileCapability::Typecheck(
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
    assert!(missing.contains(&CompileCapability::Typecheck(
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
    assert!(missing.contains(&CompileCapability::Typecheck(
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
    assert!(missing.contains(&CompileCapability::Typecheck(
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
    assert!(missing.contains(&CompileCapability::Typecheck(
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
    assert!(missing.contains(&CompileCapability::Typecheck(
        TypecheckCapability::CheckClosures
    )));
}
