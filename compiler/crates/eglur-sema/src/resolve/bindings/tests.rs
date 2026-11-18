use super::*;
use crate::resolve::tests::with_resolver;
use crate::resolve::{Resolver, ScopeKind};
use crate::{CompileCapability, CompileSupport, ResolveCapability, SemanticSource};
use eglur_hir::{FieldPattern, ItemKind, PatternKind, PatternPayload, StmtKind};

struct Support(Option<CompileCapability>);

impl CompileSupport for Support {
    fn supports(&self, capability: CompileCapability) -> bool {
        self.0 != Some(capability)
    }
}

fn resolved(
    text: &str,
    support: &dyn CompileSupport,
    inspect: impl FnOnce(&Resolver<'_, '_>, &SemanticSource<'_>),
) {
    with_resolver(text, support, |resolver, source| {
        let module = resolver.collection.module_for_file(source.file()).unwrap();
        let hir = source.hir().unwrap();
        for &item in hir.roots() {
            resolver.visit_item(module, source, hir, item).unwrap();
        }
        inspect(resolver, source);
    });
}

fn assert_site_origins(resolver: &Resolver<'_, '_>, source: &SemanticSource<'_>) {
    let module = resolver.collection.module_for_file(source.file()).unwrap();
    for (body_id, body) in source.hir().unwrap().bodies() {
        let view = resolver.pattern_bindings.body(module, body_id);
        for (pattern, value) in body.patterns() {
            match value.kind() {
                PatternKind::Binding(name) | PatternKind::Alias { name, .. } => {
                    assert_eq!(
                        view.as_ref()
                            .and_then(|view| view.binding(PatternBindingSite::Name(pattern))),
                        resolver
                            .local_by_origin
                            .get(&(module, body_id, name.origin()))
                            .copied(),
                        "{pattern:?}"
                    );
                }
                PatternKind::Struct { fields, .. }
                | PatternKind::Variant {
                    payload: PatternPayload::Named(fields),
                    ..
                } => {
                    for (field, value) in fields.iter().enumerate() {
                        let actual = view.as_ref().and_then(|view| {
                            view.binding(PatternBindingSite::Shorthand { pattern, field })
                        });
                        let expected = match value {
                            FieldPattern::Field {
                                name,
                                pattern: None,
                                ..
                            } => resolver
                                .local_by_origin
                                .get(&(module, body_id, name.origin()))
                                .copied(),
                            _ => None,
                        };
                        assert_eq!(actual, expected, "{pattern:?}, field {field}");
                    }
                }
                _ => {}
            }
        }
    }
}

#[test]
fn source_sites_preserve_origins_and_coalesced_identities() {
    for pattern in [
        "zeta @ (alpha, zeta)",
        "(same, same)",
        "(left, ..)",
        "Some(x) | Some(x)",
        "S { first, middle: (alias @ inner), last, .. }",
        "E::V { first, middle: explicit, last }",
    ] {
        resolved(
            &format!(
                "fn f(value: Unit) -> Unit = match value {{ {pattern} => () }}\nfn g(value: Unit) -> Unit = value\n"
            ),
            &Support(None),
            |resolver, source| {
                assert!(
                    source.hir().unwrap().invalid_occurrences().is_empty(),
                    "{pattern}"
                );
                assert_site_origins(resolver, source);
                let module = resolver.collection.module_for_file(source.file()).unwrap();
                for local in &resolver.locals {
                    assert_eq!(local.id.module(), module);
                    for &origin in local.origins() {
                        assert_eq!(
                            resolver
                                .local_by_origin
                                .get(&(module, local.id.body(), origin)),
                            Some(&local.id)
                        );
                    }
                }
            },
        );
    }
}

#[test]
fn root_order_follows_activation_not_origin_or_alphabetic_order() {
    resolved(
        "fn f(value: i64) -> i64 { let first = match value { (zeta, alpha) => 0 }\nlet (zulu, able) = (0, 1)\nyield first }\n",
        &Support(None),
        |resolver, source| {
            assert_site_origins(resolver, source);
            let module = resolver.collection.module_for_file(source.file()).unwrap();
            let hir = source.hir().unwrap();
            let ItemKind::Function(function) = hir.item(hir.roots()[0]).kind() else {
                panic!("function")
            };
            let body = hir.body(function.body);
            let view = resolver.pattern_bindings.body(module, body.id()).unwrap();
            let mut let_roots = Vec::new();
            for (_, statement) in body.statements() {
                if let StmtKind::Let { pattern, .. } = statement.kind() {
                    let_roots.push((*pattern, view.root(*pattern).unwrap()));
                }
            }
            let names = resolver
                .locals
                .iter()
                .map(|local| local.name.text())
                .collect::<Vec<_>>();
            assert_eq!(names, ["value", "zeta", "alpha", "first", "zulu", "able"]);
            assert_eq!(let_roots[0].1, [resolver.locals[3].id]);
            assert_eq!(
                let_roots[1].1,
                [resolver.locals[4].id, resolver.locals[5].id]
            );
            assert!(resolver.locals[3].origin().get() < resolver.locals[1].origin().get());
            let manifest_ids = view
                .bindings
                .roots
                .iter()
                .flat_map(|ids| ids.iter().copied())
                .collect::<Vec<_>>();
            assert_eq!(
                manifest_ids,
                resolver
                    .locals
                    .iter()
                    .map(|local| local.id)
                    .collect::<Vec<_>>()
            );
        },
    );
}

#[test]
fn collisions_leave_no_activated_root_or_site() {
    resolved(
        "fn f(x: i64, x: i64) -> i64 { let shadow = 0\nlet shadow = skipped\nyield x }\n",
        &Support(None),
        |resolver, source| {
            assert_site_origins(resolver, source);
            let module = resolver.collection.module_for_file(source.file()).unwrap();
            let hir = source.hir().unwrap();
            let ItemKind::Function(function) = hir.item(hir.roots()[0]).kind() else {
                panic!("function")
            };
            let view = resolver
                .pattern_bindings
                .body(module, function.body)
                .unwrap();
            assert_eq!(resolver.locals.len(), 2);
            assert_eq!(
                view.root(function.parameters[0].pattern),
                Some([resolver.locals[0].id].as_slice())
            );
            assert_eq!(view.root(function.parameters[1].pattern), None);
            let first_local = [resolver.locals[1].id];
            let mut first = true;
            for (_, statement) in hir.body(function.body).statements() {
                if let StmtKind::Let { pattern, .. } = statement.kind() {
                    let expected = first.then_some(first_local.as_slice());
                    assert_eq!(view.root(*pattern), expected);
                    first = false;
                }
            }
        },
    );
}

#[test]
fn recovered_invalid_source_does_not_invent_binding_sites() {
    resolved(
        "fn f() -> Unit { let = 0\nyield () }\nfn after(y: Unit) -> Unit = y\n",
        &Support(None),
        |resolver, source| {
            assert_site_origins(resolver, source);
            let hir = source.hir().unwrap();
            assert!(!hir.invalid_occurrences().is_empty());
            assert_eq!(
                resolver
                    .locals
                    .iter()
                    .map(|local| local.name.text())
                    .collect::<Vec<_>>(),
                ["y"]
            );
            let module = resolver.collection.module_for_file(source.file()).unwrap();
            for (body, _) in hir.bodies() {
                if body != resolver.locals[0].id.body() {
                    assert!(resolver.pattern_bindings.body(module, body).is_none());
                }
            }
        },
    );
}

#[test]
fn unavailable_and_poisoned_heads_still_publish_activated_ids() {
    for support in [
        Support(None),
        Support(Some(CompileCapability::Resolve(
            ResolveCapability::ResolvePatternScopes,
        ))),
    ] {
        resolved(
            "fn f(value: Unit) -> Unit = match value { Missing(alias @ x) => () }\n",
            &support,
            |resolver, source| {
                assert_site_origins(resolver, source);
                let module = resolver.collection.module_for_file(source.file()).unwrap();
                let locals = resolver
                    .locals
                    .iter()
                    .filter(|local| local.name.text() != "value")
                    .collect::<Vec<_>>();
                assert_eq!(locals.len(), 2);
                let view = resolver
                    .pattern_bindings
                    .body(module, locals[0].id.body())
                    .unwrap();
                assert!(
                    view.bindings
                        .roots
                        .iter()
                        .any(|ids| ids.as_ref() == [locals[0].id, locals[1].id])
                );
            },
        );
    }
}

#[test]
fn empty_roots_are_distinct_from_unactivated_patterns_and_for_names() {
    resolved(
        "fn f(_: Unit) -> Unit { for name in [] { yield () }\nyield () }\n",
        &Support(None),
        |resolver, source| {
            let module = resolver.collection.module_for_file(source.file()).unwrap();
            let hir = source.hir().unwrap();
            let ItemKind::Function(function) = hir.item(hir.roots()[0]).kind() else {
                panic!("function")
            };
            let view = resolver
                .pattern_bindings
                .body(module, function.body)
                .unwrap();
            assert_eq!(
                view.root(function.parameters[0].pattern),
                Some([].as_slice())
            );
            assert_eq!(
                view.binding(PatternBindingSite::Name(function.parameters[0].pattern)),
                None
            );
            let name = resolver
                .locals
                .iter()
                .find(|local| local.name.text() == "name")
                .unwrap();
            assert_eq!(
                resolver
                    .local_by_origin
                    .get(&(module, name.id.body(), name.origin())),
                Some(&name.id)
            );
            assert!(view.bindings.sites.is_empty());
        },
    );
}

#[test]
fn owner_views_and_missing_queries_preserve_storage() {
    with_resolver(
        "fn f(x: Unit) -> Unit = x\nfn g(y: Unit) -> Unit = y\n",
        &Support(None),
        |resolver, source| {
            let hir = source.hir().unwrap();
            let patterns = hir
                .bodies()
                .map(|(_, body)| body.patterns().next().unwrap().0)
                .collect::<Vec<_>>();
            let mut bindings = ResolvedPatternBindings::default();
            let left = ModuleId::from_index(0).unwrap();
            let right = ModuleId::from_index(2).unwrap();
            for module in [left, right] {
                let id = LocalBindingId::from_index(module, patterns[0].body(), 0).unwrap();
                bindings.insert(PatternBindingSite::Name(patterns[0]), id);
                bindings.publish_root(module, patterns[0], vec![id]);
            }
            let before = storage(&bindings);
            for module in [left, right] {
                let view = bindings.body(module, patterns[0].body()).unwrap();
                let expected = LocalBindingId::from_index(module, patterns[0].body(), 0).unwrap();
                assert_eq!(
                    view.binding(PatternBindingSite::Name(patterns[0])),
                    Some(expected)
                );
                assert_eq!(view.root(patterns[0]), Some([expected].as_slice()));
                assert_eq!(view.binding(PatternBindingSite::Name(patterns[1])), None);
                assert_eq!(view.root(patterns[1]), None);
                assert_eq!(
                    view.binding(PatternBindingSite::Shorthand {
                        pattern: patterns[0],
                        field: usize::MAX
                    }),
                    None
                );
            }
            assert!(
                bindings
                    .body(
                        ModuleId::from_index(u32::MAX as usize).unwrap(),
                        patterns[0].body()
                    )
                    .is_none()
            );
            assert!(bindings.body(left, patterns[1].body()).is_none());
            assert_eq!(storage(&bindings), before);
            assert!(resolver.locals.is_empty());
        },
    );
}

fn storage(bindings: &ResolvedPatternBindings) -> (usize, usize, usize, usize, usize) {
    let mut size = (bindings.modules.capacity(), 0, 0, 0, 0);
    for bodies in &bindings.modules {
        size.1 += bodies.capacity();
        for body in bodies.iter().flatten() {
            size.2 += body.slots.capacity();
            size.3 += body.sites.capacity();
            size.4 += body.roots.capacity();
        }
    }
    size
}

fn backing_bytes(bindings: &ResolvedPatternBindings) -> usize {
    let mut bytes = bindings.modules.capacity() * size_of::<Vec<Option<BodyBindings>>>();
    for bodies in &bindings.modules {
        bytes += bodies.capacity() * size_of::<Option<BodyBindings>>();
        for body in bodies.iter().flatten() {
            bytes += body.slots.capacity() * size_of::<PatternSlots>();
            bytes += body.sites.capacity() * size_of::<SiteBindings>();
            bytes += body.roots.capacity() * size_of::<Box<[LocalBindingId]>>();
            for site in &body.sites {
                if let SiteBindings::Shorthands(fields) = site {
                    bytes += fields.capacity() * size_of::<Option<LocalBindingId>>();
                }
            }
            bytes += body
                .roots
                .iter()
                .map(|ids| std::mem::size_of_val(ids.as_ref()))
                .sum::<usize>();
        }
    }
    bytes
}

#[test]
fn source_growth_allocates_body_pattern_slots_not_file_origin_rows() {
    for sparse in [false, true] {
        for count in [1, 8, 64, 256] {
            let text = (0..count)
                .map(|index| {
                    if sparse && index + 1 < count {
                        format!("fn f{index}() -> Unit = ()\n")
                    } else {
                        format!("fn f{index}(x: Unit) -> Unit {{ let y = x\nyield y }}\n")
                    }
                })
                .collect::<String>();
            resolved(&text, &Support(None), |resolver, source| {
                assert_site_origins(resolver, source);
                let hir = source.hir().unwrap();
                let module = resolver.collection.module_for_file(source.file()).unwrap();
                let mut patterns = 0;
                let mut slots = 0;
                let mut roots = 0;
                for (id, body) in hir.bodies() {
                    patterns += body.patterns().len();
                    if let Some(view) = resolver.pattern_bindings.body(module, id) {
                        slots += view.bindings.slots.len();
                        assert!(view.bindings.slots.len() <= body.patterns().len());
                        roots += view.bindings.roots.len();
                    }
                }
                assert!(slots <= patterns);
                assert_eq!(roots, resolver.locals.len());
                assert!(
                    resolver.pattern_bindings.modules[module.index()].len() <= hir.bodies().len()
                );
                let size = storage(&resolver.pattern_bindings);
                eprintln!(
                    "sparse={sparse} bodies={count} patterns={patterns} slots={slots} roots={roots} capacities={size:?} backing_bytes={} slot_size={} site_size={} occurrence_size={}",
                    backing_bytes(&resolver.pattern_bindings),
                    size_of::<PatternSlots>(),
                    size_of::<SiteBindings>(),
                    size_of::<crate::resolve::BindingOccurrence>()
                );
            });
        }
    }
}

#[test]
fn repeated_empty_parameter_records_do_not_create_an_error() {
    with_resolver(
        "fn f(_: Unit) -> Unit = ()\n",
        &Support(None),
        |resolver, source| {
            let hir = source.hir().unwrap();
            let module = resolver.collection.module_for_file(source.file()).unwrap();
            let ItemKind::Function(function) = hir.item(hir.roots()[0]).kind() else {
                panic!("function")
            };
            let body = hir.body(function.body);
            resolver
                .push_scope(module, body.id(), ScopeKind::CallableParameters)
                .unwrap();
            let parameters = vec![function.parameters[0].clone(); 3];
            resolver
                .visit_parameters(module, source, hir, body, &parameters)
                .unwrap();
            let view = resolver.pattern_bindings.body(module, body.id()).unwrap();
            assert_eq!(view.root(parameters[0].pattern), Some([].as_slice()));
            assert_eq!(view.bindings.roots.len(), 1);
        },
    );
}
