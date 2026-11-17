use super::*;

fn residual_environment() -> ExternalNameEnvironment {
    let owner = module(&["builtin"]);
    let declarations = ["Some", "None", "Ok", "Err"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            ExternalDeclaration::new(
                ExternalDefId::new(format!("constructor.{index}")).unwrap(),
                owner.clone(),
                NameKey::new(name, false).unwrap(),
                DeclarationKind::IntrinsicConstructor,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let prelude = declarations
        .iter()
        .map(|declaration| declaration.id().clone())
        .collect::<Vec<_>>();
    ExternalNameEnvironment::new([owner], declarations, prelude).unwrap()
}

#[test]
fn residual_pattern_lookup_uses_prelude_identity_not_shadow_bindings() {
    let external = residual_environment();
    for (index, head) in ["Some", "None", "Ok", "Err"].into_iter().enumerate() {
        for pattern in [
            head.to_owned(),
            format!("{head}(value)"),
            format!("{head} {{ value }}"),
        ] {
            for source in [
                format!(
                    "fn f({head}: i64, input: i64) -> i64 = match input {{ {pattern} => {head} }}\n"
                ),
                format!(
                    "fn f(input: i64) -> i64 {{ let {head} = 1\nyield match input {{ {pattern} => {head} }} }}\n"
                ),
                format!(
                    "fn {head}() -> i64 = 1\nfn f(input: i64) -> i64 = match input {{ {pattern} => {head}() }}\n"
                ),
                format!(
                    "import dep::shadow as {head}\nfn f(input: i64) -> i64 = match input {{ {pattern} => {head}() }}\n"
                ),
            ] {
                let analysis = analyze(
                    vec![
                        ("dep.eglur", "pub fn shadow() -> i64 = 1\n"),
                        ("main.eglur", &source),
                    ],
                    &[&[0], &[1]],
                    &CapabilitySet::resolver(true),
                    Some(&external),
                );
                assert!(
                    codes(&analysis.diagnostics).is_empty(),
                    "{source}: {:?}",
                    analysis.diagnostics
                );
                let targets = analysis
                    .names
                    .facts()
                    .filter_map(|(key, fact)| match (key, fact) {
                        (
                            ResolutionKey::Pattern { .. },
                            SemanticFact::Known(ResolvedName::Declarations(targets)),
                        ) => Some(targets),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(targets.len(), 1, "{source}");
                assert_eq!(
                    targets[0].as_ref(),
                    [CanonicalTarget::External(
                        ExternalDefId::new(format!("constructor.{index}")).unwrap()
                    )],
                    "{source}"
                );
                assert!(
                    analysis.names.facts().any(|(key, fact)| {
                        matches!(key, ResolutionKey::Expression { .. })
                            && matches!(fact, SemanticFact::Known(ResolvedName::Local(_)))
                    }),
                    "scrutinee must keep ordinary local resolution: {source}"
                );
            }
        }
    }
}

#[test]
fn residual_pattern_lookup_preserves_unavailable_phases() {
    for removed in [
        ResolveCapability::ResolveLexicalValues,
        ResolveCapability::ResolveModuleValues,
        ResolveCapability::ResolveTrustedExternalNames,
        ResolveCapability::ResolvePatternScopes,
    ] {
        let mut support = CapabilitySet::resolver(true);
        support.0.remove(&CompileCapability::Resolve(removed));
        let external = residual_environment();
        let analysis = analyze(
            vec![(
                "main.eglur",
                "fn f(input: i64) -> i64 = match input { None => 0 }\n",
            )],
            &[&[0]],
            &support,
            Some(&external),
        );
        assert!(codes(&analysis.diagnostics).is_empty(), "{removed:?}");
        assert!(
            analysis
                .names
                .locals()
                .iter()
                .all(|local| local.name().text() != "None")
        );
        assert!(
            !analysis.names.facts().any(|(key, fact)| {
                matches!(
                    (key, fact),
                    (ResolutionKey::Pattern { .. }, SemanticFact::Known(_))
                )
            }),
            "{removed:?}"
        );
        assert!(
            analysis.demands.iter().any(|demand| {
                demand.capability() == CompileCapability::Resolve(removed)
                    && demand.disposition() == DemandDisposition::Missing
            }),
            "{removed:?}"
        );
    }
}

#[test]
fn residual_pattern_lookup_does_not_erase_import_diagnostics() {
    let external = residual_environment();
    let analysis = analyze(
        vec![
            ("dep.eglur", "pub fn present() -> i64 = 1\n"),
            (
                "main.eglur",
                "import dep::missing as None\nfn f(input: i64) -> i64 = match input { None => 0 }\n",
            ),
        ],
        &[&[0], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );
    assert_eq!(codes(&analysis.diagnostics), ["IMPORT_NOT_FOUND"]);
    assert!(analysis.names.facts().any(|(key, fact)| {
        matches!(
            (key, fact),
            (
                ResolutionKey::Pattern { .. },
                SemanticFact::Known(ResolvedName::Declarations(_))
            )
        )
    }));
}

#[test]
fn residual_pattern_lookup_does_not_erase_binding_diagnostics() {
    let external = residual_environment();
    let analysis = analyze(
        vec![(
            "main.eglur",
            "fn f(None: i64, None: i64, input: i64) -> i64 = match input { None => 0 }\n",
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );
    assert_eq!(
        codes(&analysis.diagnostics),
        ["LOCAL_BINDING_REDECLARATION"]
    );
    assert!(analysis.names.facts().any(|(key, fact)| {
        matches!(
            (key, fact),
            (
                ResolutionKey::Pattern { .. },
                SemanticFact::Known(ResolvedName::Declarations(_))
            )
        )
    }));
}

#[test]
fn residual_heads_do_not_fall_back_to_locals_when_the_prelude_is_empty() {
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            "fn f(None: i64, input: i64) -> i64 = match input { None => None }\n",
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );
    assert_eq!(codes(&analysis.diagnostics), ["NAME_NOT_FOUND"]);
}
