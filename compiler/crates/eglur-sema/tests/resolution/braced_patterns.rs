use super::*;

#[test]
fn braced_pattern_paths_select_canonical_structs_and_owned_variants() {
    let external = empty_external();
    for (head, expected_kind, expected_name) in [
        ("Record", DeclarationKind::Struct, "Record"),
        ("Imported", DeclarationKind::Struct, "Record"),
        ("dep::Record", DeclarationKind::Struct, "Record"),
        ("bridge::Shared", DeclarationKind::Struct, "Record"),
        ("dep::Choice::Named", DeclarationKind::EnumVariant, "Named"),
        (
            "bridge::Choice::Named",
            DeclarationKind::EnumVariant,
            "Named",
        ),
    ] {
        let main = format!(
            "import dep::Record\nimport dep::Record as Imported\nfn f(input: i64) -> i64 = match input {{ {head} {{ value }} => value }}\n"
        );
        let analysis = analyze(
            vec![
                (
                    "bridge.eglur",
                    "pub import dep::Record as Shared\npub import dep::Choice\n",
                ),
                (
                    "dep.eglur",
                    "pub struct Record { pub field value: i64 }\npub enum Choice { Named { value: i64 } }\n",
                ),
                ("main.eglur", &main),
            ],
            &[&[1], &[0], &[2]],
            &CapabilitySet::resolver(true),
            Some(&external),
        );
        assert!(codes(&analysis.diagnostics).is_empty(), "{head}");
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
        assert_eq!(targets.len(), 1, "{head}");
        let [CanonicalTarget::Source(id)] = targets[0].as_ref() else {
            panic!("pattern head did not select one source declaration: {head}");
        };
        let declaration = analysis.declarations.declaration(*id).unwrap();
        assert_eq!(declaration.kind(), expected_kind, "{head}");
        assert_eq!(declaration.name().unwrap().text(), expected_name, "{head}");
    }
}

#[test]
fn qualified_external_braced_pattern_ignores_the_value_namespace() {
    let geometry = module(&["geometry"]);
    let point_type = ExternalDefId::new("geometry.Point.type").unwrap();
    let declarations = [
        ExternalDeclaration::new(
            point_type.clone(),
            geometry.clone(),
            NameKey::new("Point", false).unwrap(),
            DeclarationKind::Struct,
        )
        .unwrap(),
        ExternalDeclaration::new(
            ExternalDefId::new("geometry.Point.value").unwrap(),
            geometry.clone(),
            NameKey::new("Point", false).unwrap(),
            DeclarationKind::Function,
        )
        .unwrap(),
    ];
    let external = ExternalNameEnvironment::new([geometry], declarations, []).unwrap();
    let analysis = analyze(
        vec![(
            "main.eglur",
            "fn f(input: i64) -> i64 = match input { geometry::Point { value } => value }\n",
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(analysis.names.facts().any(|(key, fact)| {
        matches!(
            (key, fact),
            (
                ResolutionKey::Pattern { .. },
                SemanticFact::Known(ResolvedName::Declarations(targets)),
            ) if matches!(targets.as_ref(), [CanonicalTarget::External(target)]
                if target == &point_type)
        )
    }));
}

#[test]
fn qualified_braced_pattern_does_not_promote_an_external_function() {
    let geometry = module(&["geometry"]);
    let function = ExternalDeclaration::new(
        ExternalDefId::new("geometry.Point.value").unwrap(),
        geometry.clone(),
        NameKey::new("Point", false).unwrap(),
        DeclarationKind::Function,
    )
    .unwrap();
    let external = ExternalNameEnvironment::new([geometry], [function], []).unwrap();
    let analysis = analyze(
        vec![(
            "main.eglur",
            "fn f(input: i64) -> i64 = match input { geometry::Point { value } => value }\n",
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(codes(&analysis.diagnostics), ["NAME_NOT_FOUND"]);
}
