use super::{CapabilitySet, analyze, codes, empty_external};
use eglur_sema::{
    CompileCapability, DemandDisposition, ResolutionKey, ResolveCapability, ResolvedName,
    ScopeKind, SemanticFact,
};

#[test]
fn parser_budgeted_expression_forms_resolve_on_the_default_stack() {
    let external = empty_external();
    for (prefix, suffix, depth) in [
        ("f(", ")", 255),
        ("f(value = ", ")", 255),
        ("f::[i64](", ")", 255),
        ("f[f32](", ")", 255),
        ("[", "]", 255),
        ("[x: ", "]", 255),
        ("[", ": x]", 255),
        ("(", ", x)", 255),
        ("{ yield ", " }", 255),
        ("{ || ", " }", 255),
        ("if true { yield ", " } else { yield x }", 127),
        ("match x { y => ", " }", 127),
        ("{ let y = ", "\nyield y }", 255),
        ("{ while true { yield ", " }\nyield x }", 85),
        ("{ for y in x { yield ", " }\nyield x }", 85),
    ] {
        let source = format!(
            "fn f(x: i64) -> i64 = {}x{}\n",
            prefix.repeat(depth),
            suffix.repeat(depth),
        );
        let analysis = analyze(
            vec![("main.eglur", &source)],
            &[&[0]],
            &CapabilitySet::resolver(true),
            Some(&external),
        );
        assert!(codes(&analysis.diagnostics).is_empty(), "{prefix}");
        assert!(
            analysis
                .names
                .facts()
                .any(|(_, fact)| { matches!(fact, SemanticFact::Known(ResolvedName::Local(_))) })
        );
        assert!(
            analysis
                .demands
                .iter()
                .all(|demand| { demand.disposition() == DemandDisposition::Satisfied }),
            "{prefix}",
        );
    }
}

#[test]
fn unbounded_postfix_and_grouped_assignment_paths_resolve() {
    let external = empty_external();
    for expression in [
        format!("x{}", ".member".repeat(4_000)),
        format!("x{}", "()".repeat(4_000)),
        format!("x{}", ".0".repeat(4_000)),
    ] {
        let source = format!("fn f(x: i64) -> i64 = {expression}\n");
        let analysis = analyze(
            vec![("main.eglur", &source)],
            &[&[0]],
            &CapabilitySet::resolver(true),
            Some(&external),
        );
        assert!(codes(&analysis.diagnostics).is_empty());
        assert!(
            analysis
                .names
                .facts()
                .any(|(_, fact)| { matches!(fact, SemanticFact::Known(ResolvedName::Local(_))) })
        );
    }

    let source = format!(
        "fn f!() -> Unit {{ {}missing{} = 1\nyield () }}\n",
        "(".repeat(254),
        ")".repeat(254),
    );
    let analysis = analyze(
        vec![("main.eglur", &source)],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );
    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(
        analysis
            .names
            .facts()
            .any(|(_, fact)| { matches!(fact, SemanticFact::NotApplicable) })
    );
}

#[test]
fn nested_shadow_initializers_and_siblings_keep_local_identity() {
    let depth = 120;
    let source = format!(
        "fn f(x: i64) -> i64 = ({}x{}, x)\n",
        "{ let x = x\nyield ".repeat(depth),
        " }".repeat(depth),
    );
    let external = empty_external();
    let analysis = analyze(
        vec![("main.eglur", &source)],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );
    assert!(codes(&analysis.diagnostics).is_empty());
    let locals = analysis.names.locals();
    assert_eq!(locals.len(), depth + 1);
    for (index, local) in locals.iter().enumerate() {
        let references = analysis.names.facts().filter(|(key, fact)| {
            matches!(key, ResolutionKey::Expression { .. })
                && matches!(fact, SemanticFact::Known(ResolvedName::Local(id)) if *id == local.id())
        }).count();
        assert_eq!(references, if index == 0 { 2 } else { 1 });
    }
    let scopes = analysis.names.scopes();
    assert_eq!(scopes.len(), depth + 1);
    for pair in scopes.windows(2) {
        assert_eq!(pair[1].parent(), Some(pair[0].id()));
    }
}

#[test]
fn loop_match_and_lambda_scopes_do_not_leak_to_siblings() {
    let external = empty_external();
    let source = concat!(
        "fn f(x: i64) -> i64 {\n",
        "for x in x { _ = x }\n",
        "_ = match x { y if y => { |z| (y, z) }, _ => x }\n",
        "yield x\n}\n",
    );
    for patterns in [true, false] {
        let mut support = CapabilitySet::resolver(true);
        if !patterns {
            support.0.remove(&CompileCapability::Resolve(
                ResolveCapability::ResolvePatternScopes,
            ));
        }
        let analysis = analyze(
            vec![("main.eglur", source)],
            &[&[0]],
            &support,
            Some(&external),
        );
        assert!(codes(&analysis.diagnostics).is_empty());
        let locals = analysis.names.locals();
        assert_eq!(locals.len(), 4);
        let outer_x = locals[0].id();
        let loop_x = locals[1].id();
        let references = |local| {
            analysis.names.facts().filter(|(key, fact)| {
            matches!(key, ResolutionKey::Expression { .. })
                && matches!(fact, SemanticFact::Known(ResolvedName::Local(id)) if *id == local)
        }).count()
        };
        assert_eq!(references(outer_x), 4);
        assert_eq!(references(loop_x), 1);
        assert_eq!(references(locals[2].id()), if patterns { 2 } else { 0 });
        assert_eq!(references(locals[3].id()), 1);
        assert_eq!(
            analysis
                .names
                .scopes()
                .iter()
                .filter(|scope| scope.kind() == ScopeKind::MatchArm)
                .count(),
            2
        );
        assert_eq!(
            analysis
                .demands
                .iter()
                .filter(|demand| demand.disposition() == DemandDisposition::Missing)
                .count(),
            if patterns { 0 } else { 2 }
        );
    }
}

#[test]
fn nested_poison_and_collision_do_not_hide_sibling_lookups() {
    let external = empty_external();
    let source = concat!(
        "fn f(x: i64) -> i64 = ({\n",
        "let y = f(missing_left)\n",
        "let y = suppressed_initializer\n",
        "yield { || y }\n",
        "}, x, missing_right)\n",
    );
    let analysis = analyze(
        vec![("main.eglur", source)],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );
    let diagnostics = codes(&analysis.diagnostics);
    assert_eq!(diagnostics.len(), 3);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|&&code| code == "NAME_NOT_FOUND")
            .count(),
        2
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|&&code| code == "LOCAL_BINDING_REDECLARATION")
            .count(),
        1
    );
    assert_eq!(analysis.names.locals().len(), 2);
    let outer = analysis.names.locals()[0].id();
    assert!(analysis.names.facts().any(|(key, fact)| {
        matches!(key, ResolutionKey::Expression { .. })
            && matches!(fact, SemanticFact::Known(ResolvedName::Local(id)) if *id == outer)
    }));
    assert_eq!(
        analysis
            .names
            .facts()
            .filter(|(key, fact)| {
                matches!(key, ResolutionKey::Expression { .. })
                    && matches!(fact, SemanticFact::Poisoned(_))
            })
            .count(),
        3
    );
}
