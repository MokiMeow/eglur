use super::super::tests::with_resolver;
use super::*;
use crate::owned_poison::test_support::{Work, reset, work};
use crate::{CompileCapability, CompileSupport, ResolveCapability};
use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticStore, PrimarySpan, Severity,
};

struct Support(u8);
impl CompileSupport for Support {
    fn supports(&self, capability: CompileCapability) -> bool {
        if capability == CompileCapability::Resolve(ResolveCapability::ResolveAttributePaths) {
            return false;
        }
        match self.0 {
            1 => capability != CompileCapability::Resolve(ResolveCapability::ResolveLexicalValues),
            2 => capability != CompileCapability::Resolve(ResolveCapability::ResolveModuleValues),
            _ => true,
        }
    }
}

fn texts(count: usize) -> Vec<(String, String)> {
    let methods = (0..count)
        .map(|i| format!("fn m{i}() -> i64 = missing_{i}\n"))
        .collect::<String>();
    let fields = (0..count)
        .map(|i| format!("field f{i}: i64 = missing_{i}\n"))
        .collect::<String>();
    let healthy = (0..count)
        .map(|i| format!("fn m{i}() -> i64 = {i}\n"))
        .collect::<String>();
    let repeated = (0..count)
        .map(|i| format!("fn m{i}() -> i64 = missing\n"))
        .collect::<String>();
    let types = (0..count)
        .map(|i| format!("Missing{i}"))
        .collect::<Vec<_>>();
    let tuple = if count == 0 {
        "Unit".into()
    } else {
        format!("({},)", types.join(","))
    };
    let args = if count == 0 {
        "MissingHead".into()
    } else {
        format!("MissingHead[{}]", types.join(","))
    };
    let bounds = if count == 0 {
        "T".into()
    } else {
        format!("T: {}", types.join("+"))
    };
    let mut cases = vec![
        (
            "struct_methods".into(),
            format!("struct S {{ {methods} }}\n"),
        ),
        (
            "property_methods".into(),
            format!("struct S {{ prop value: i64 {{ {methods} }} }}\n"),
        ),
        (
            "nested_properties".into(),
            format!(
                "struct S {{ {} }}\n",
                (0..count)
                    .map(|i| format!(
                        "prop p{i}: i64 {{ fn get_p{i}(self: &Self) -> i64 = missing_{i} }}\n"
                    ))
                    .collect::<String>()
            ),
        ),
        ("struct_fields".into(), format!("struct S {{ {fields} }}\n")),
        ("trait_methods".into(), format!("trait T {{ {methods} }}\n")),
        (
            "impl_methods".into(),
            format!("struct S {{}}\ntrait T {{}}\nimpl T for S {{ {methods} }}\n"),
        ),
        ("healthy".into(), format!("struct S {{ {healthy} }}\n")),
        (
            "repeated_lookup".into(),
            format!("struct S {{ {repeated} }}\n"),
        ),
        ("tuple_types".into(), format!("type A = {tuple}\n")),
        (
            "function_types".into(),
            format!("type A = Fn({}) -> MissingResult\n", types.join(",")),
        ),
        ("applied_types".into(), format!("type A = {args}\n")),
        ("generic_bounds".into(), format!("trait T[{bounds}] {{}}\n")),
        (
            "enum_types".into(),
            if count == 0 {
                "enum E {{}}\n".replace("{{}}", "{}")
            } else {
                format!("enum E {{\nA({})\n}}\n", types.join(","))
            },
        ),
        (
            "attributes".into(),
            format!(
                "{}struct S {{ {healthy} }}\n",
                (0..count)
                    .map(|i| format!("#[outer{i}(flag = true)]\n"))
                    .collect::<String>()
            ),
        ),
        (
            "mixed_attributes".into(),
            format!(
                "{}struct S {{ {methods} }}\n",
                (0..count)
                    .map(|i| format!("#[outer{i}(flag = true)]\n"))
                    .collect::<String>()
            ),
        ),
    ];
    for width in [1, 8] {
        let prefix = (0..width)
            .map(|i| format!("fn damaged{i}() -> Unit = ,\n"))
            .collect::<String>();
        cases.push((
            format!("shared_{width}"),
            format!("{prefix}struct S {{ {methods} }}\n"),
        ));
    }
    for (_, text) in &mut cases {
        text.push_str("fn after(x: i64) -> i64 = x\n");
    }
    cases
}

#[test]
fn source_item_siblings_complete_once_and_keep_required_facts_and_owner_state() {
    for count in [3usize, 0, 1, 2, 8, 32, 128] {
        for (mode, text) in texts(count) {
            for profile in 0..3 {
                with_resolver(&text, &Support(profile), |resolver, source| {
                    reset();
                    let module = resolver.collection.module_for_file(source.file()).unwrap();
                    let hir = source.hir().unwrap();
                    for &item in hir.roots() {
                        resolver.visit_item(module, source, hir, item).unwrap();
                    }
                    let growing = matches!(
                        mode.as_str(),
                        "struct_methods"
                            | "struct_fields"
                            | "trait_methods"
                            | "impl_methods"
                            | "property_methods"
                            | "nested_properties"
                            | "repeated_lookup"
                            | "mixed_attributes"
                            | "shared_1"
                            | "shared_8"
                    );
                    let poisons = if growing && profile == 0 { count } else { 0 };
                    assert_eq!(
                        work(),
                        Work {
                            pairs: usize::from(poisons > 1),
                            includes: if poisons > 2 { poisons - 1 } else { 0 },
                            finishes: usize::from(poisons > 2),
                        },
                        "{mode}/{count}/{profile}"
                    );
                    assert!(resolver.current_owner.is_none());
                    assert!(resolver.active_scopes.is_empty());
                    if growing && profile == 0 {
                        let poisoned = resolver
                            .facts
                            .iter()
                            .filter(|(_, fact)| matches!(fact, crate::SemanticFact::Poisoned(_)))
                            .count();
                        assert_eq!(poisoned, count);
                    }
                    let snapshot = std::mem::replace(resolver.diagnostics, DiagnosticStore::new())
                        .finalize(None);
                    let expected = if mode == "shared_1" || mode == "shared_8" {
                        (0..if mode == "shared_1" { 1 } else { 8 })
                            .map(|i| format!("SYNTAX_{i}"))
                            .collect::<Vec<_>>()
                    } else if growing && profile == 0 {
                        vec!["NAME_NOT_FOUND".to_owned(); count]
                    } else {
                        Vec::new()
                    };
                    assert_eq!(
                        snapshot
                            .diagnostics()
                            .iter()
                            .map(|d| d.code().as_str().to_owned())
                            .collect::<Vec<_>>(),
                        expected,
                        "{mode}/{count}/{profile}"
                    );
                });
            }
        }
    }
}

#[test]
fn item_children_forward_empty_seed_and_stop_after_a_foreign_join() {
    with_resolver(
        "struct S {}\nfn first() -> i64 = missing_first\nfn second() -> i64 = missing_second\n",
        &Support(0),
        |resolver, source| {
            let module = resolver.collection.module_for_file(source.file()).unwrap();
            let hir = source.hir().unwrap();
            let owner = resolver
                .collection
                .modules()
                .flat_map(|m| m.declarations())
                .find(|d| d.name().is_some_and(|n| n.text() == "S"))
                .unwrap()
                .id();
            resolver.current_owner = Some(owner);
            let identity =
                super::super::ResolvedName::TypeMemberOwner(crate::CanonicalTarget::Source(owner));
            let forwarded = resolver
                .visit_item_children(
                    module,
                    source,
                    hir,
                    VisitOutcome::Resolved(identity.clone()),
                    &[],
                )
                .unwrap();
            assert!(matches!(forwarded, VisitOutcome::Resolved(found) if found == identity));
            let not_applicable = resolver
                .visit_item_children(module, source, hir, VisitOutcome::NotApplicable, &[])
                .unwrap();
            assert!(matches!(not_applicable, VisitOutcome::NotApplicable));
            let unperformed = resolver
                .visit_item_children(module, source, hir, VisitOutcome::Unperformed, &[])
                .unwrap();
            assert!(matches!(unperformed, VisitOutcome::Unperformed));

            let mut foreign_store = DiagnosticStore::new();
            let diagnostic = Diagnostic::new(
                DiagnosticCode::new("FOREIGN_ROOT").unwrap(),
                Severity::Error,
                DiagnosticPhase::new("resolve").unwrap(),
                "independent foreign root",
                PrimarySpan::Unavailable,
            );
            let foreign = foreign_store
                .record_failure(DiagnosticFailure::Root(
                    DiagnosticRoot::boundary(BoundaryRootKey::new("foreign").unwrap(), diagnostic)
                        .unwrap(),
                ))
                .unwrap();
            reset();
            let error = resolver
                .visit_item_children(
                    module,
                    source,
                    hir,
                    VisitOutcome::Poisoned(foreign),
                    &hir.roots()[1..],
                )
                .err();
            assert_eq!(
                error,
                Some(NameResolutionError::DiagnosticStore(
                    eglur_diagnostics::DiagnosticStoreError::ForeignPoison
                ))
            );
            assert_eq!(resolver.current_owner, Some(owner));
            assert!(resolver.active_scopes.is_empty());
            assert_eq!(resolver.facts.iter().count(), 1);
            assert_eq!(work().finishes, 0);
            let snapshot =
                std::mem::replace(resolver.diagnostics, DiagnosticStore::new()).finalize(None);
            assert_eq!(snapshot.diagnostics().len(), 1);
            assert_eq!(snapshot.diagnostics()[0].code().as_str(), "NAME_NOT_FOUND");
        },
    );
}
