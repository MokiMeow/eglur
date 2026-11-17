use super::*;
use crate::signatures::resolver::tests::with_resolver;

#[test]
fn terminal_failed_bounds_publish_poisoned_occurrences() {
    for text in [
        "trait Has[A] { type Item }\nfn bad[T:Has[T::Item]]() -> Unit = ()",
        "fn bad[T:Missing]() -> Unit = ()",
    ] {
        with_resolver(&[text], |resolver, sources| {
            resolver.resolve_source_generics().unwrap();
            let parameter = resolver
                .collection
                .modules()
                .flat_map(|module| module.declarations())
                .find(|declaration| {
                    declaration.kind() == DeclarationKind::GenericParameter
                        && declaration.name().is_some_and(|name| name.text() == "T")
                })
                .unwrap()
                .id();
            let bound = resolver
                .source_generic_parameter(parameter)
                .unwrap()
                .1
                .bounds[0];
            assert!(
                matches!(
                    resolver
                        .occurrences
                        .state(&TypeOccurrenceId::new(sources[0].file(), bound)),
                    FactState::Poisoned(_)
                ),
                "{text}"
            );
        });
    }
}

#[test]
fn blocked_construction_retains_independent_error_provenance() {
    for body in ["(T::Item,Missing)", "(Missing,T::Item)"] {
        let text = format!("trait Has[A] {{ type Item }}\nfn bad[T:Has[{body}]]() -> Unit = ()");
        with_resolver(&[&text], |resolver, sources| {
            let parameter = resolver
                .collection
                .modules()
                .flat_map(|module| module.declarations())
                .find(|declaration| {
                    declaration.kind() == DeclarationKind::GenericParameter
                        && declaration.name().is_some_and(|name| name.text() == "T")
                })
                .unwrap()
                .id();
            let owner = resolver
                .collection
                .declaration(parameter)
                .unwrap()
                .parent()
                .unwrap();
            let span = sources[0]
                .origin_span(resolver.collection.declaration(owner).unwrap().origin())
                .unwrap();
            let checkpoint = resolver.diagnostics.checkpoint();
            resolver.resolve_source_generics().unwrap();
            let FactState::Poisoned(poison) = resolver
                .generic_parameters
                .state(&GenericParameterId::Source(parameter))
            else {
                panic!("failed source parameter");
            };
            let poison = poison.clone();
            let expected = resolver
                .diagnostics
                .roots_since_within(&checkpoint, span)
                .unwrap()
                .unwrap();
            resolver
                .diagnostics
                .select_preceding_roots(&poison, &expected)
                .unwrap();
            let store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
            let snapshot = store.finalize(None);
            let codes = snapshot
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.code().as_str())
                .collect::<BTreeSet<_>>();
            assert_eq!(
                codes,
                BTreeSet::from(["GENERIC_BOUND_REFERENCE_CYCLE", "NAME_NOT_FOUND"])
            );
        });
    }
}

#[test]
fn complete_projection_recovery_does_not_depend_on_parameter_validation_order() {
    for parameters in [
        "T:Uses[U::Item],U:Has[Erase[Missing]]",
        "U:Has[Erase[Missing]],T:Uses[U::Item]",
    ] {
        let text = format!(
            "trait Has[A] {{ type Item }}\ntrait Uses[A] {{}}\ntype Erase[A] = i64\nfn bad[{parameters}]() -> Unit = ()"
        );
        with_resolver(&[&text], |resolver, _| {
            resolver.resolve_source_generics().unwrap();
            for declaration in resolver
                .collection
                .modules()
                .flat_map(|module| module.declarations())
                .filter(|declaration| declaration.kind() == DeclarationKind::GenericParameter)
            {
                let name = declaration.name().unwrap().text();
                let state = resolver
                    .generic_parameters
                    .state(&GenericParameterId::Source(declaration.id()));
                if name == "T" {
                    assert!(matches!(state, FactState::Known(_)), "{text}");
                }
                if name == "U" {
                    assert!(matches!(state, FactState::Poisoned(_)), "{text}");
                }
            }
        });
    }
}

#[test]
fn sparse_associated_names_do_not_rescan_every_declared_head() {
    for count in [8, 32, 128] {
        let traits = (0..count)
            .map(|i| format!("trait Field{i} {{ type Item{i} }}\n"))
            .collect::<String>();
        let bounds = (0..count)
            .map(|i| format!("Field{i}"))
            .collect::<Vec<_>>()
            .join("+");
        let uses = (0..count)
            .map(|i| format!("T::Item{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let text = format!(
            "{traits}trait Uses[A] {{}}\nfn valid[T:{bounds}+Uses[({uses})]]() -> Unit = ()"
        );
        with_resolver(&[&text], |resolver, _| {
            resolver.resolve_source_generics().unwrap();
            assert!(!resolver.diagnostics.has_errors());
            assert_eq!(resolver.source_bounds.work.head_probes, count);
            assert_eq!(resolver.source_bounds.work.bound_starts, count + 1);
            assert_eq!(resolver.source_bounds.work.cohort_starts, count + 1);
            let edges = resolver
                .source_bounds
                .nodes
                .iter()
                .map(|node| node.dependencies.len())
                .sum::<usize>();
            assert!(edges <= 3 * count + 2, "{edges}");
        });
    }
}

#[test]
fn many_associated_names_share_one_normalized_reference_cohort() {
    for count in [8, 32, 128] {
        let names = (0..count)
            .map(|i| format!("type Item{i}\n"))
            .collect::<String>();
        let bounds = vec!["Has[i64]"; count].join("+");
        let uses = (0..count)
            .map(|i| format!("T::Item{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let text = format!(
            "trait Has[A] {{ {names} }}\ntrait Uses[A] {{}}\nfn bad[T:{bounds}+Uses[({uses})]]() -> Unit = ()"
        );
        with_resolver(&[&text], |resolver, _| {
            resolver.resolve_source_generics().unwrap();
            assert!(resolver.diagnostics.has_errors());
            assert_eq!(resolver.source_bounds.work.head_probes, count);
            assert_eq!(resolver.source_bounds.work.bound_starts, count + 1);
            assert_eq!(resolver.source_bounds.work.cohort_starts, 2);
            let edges = resolver
                .source_bounds
                .nodes
                .iter()
                .map(|node| node.dependencies.len())
                .sum::<usize>();
            assert!(edges <= 3 * count + 2, "{edges}");
        });
    }
}

#[test]
fn long_forward_dependency_chains_complete_without_recursive_bound_calls() {
    for count in [8, 32, 256] {
        let parameters = (0..count)
            .map(|i| {
                if i + 1 == count {
                    format!("T{i}:Has[i64]")
                } else {
                    format!("T{i}:Has[T{}::Item]", i + 1)
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let text = format!("trait Has[A] {{ type Item }}\nfn valid[{parameters}]() -> Unit = ()");
        with_resolver(&[&text], |resolver, _| {
            resolver.resolve_source_generics().unwrap();
            assert!(!resolver.diagnostics.has_errors());
            assert_eq!(resolver.source_bounds.work.bound_starts, count);
            assert_eq!(resolver.source_bounds.work.cohort_starts, count);
            let edges = resolver
                .source_bounds
                .nodes
                .iter()
                .map(|node| node.dependencies.len())
                .sum::<usize>();
            assert_eq!(edges, 2 * count - 1);
        });
    }
}

#[test]
fn construction_cycle_blame_is_independent_of_requested_parameter_order() {
    let text = "trait Has[A] { type Item }\nfn bad[T:Has[U::Item],U:Has[V::Item],V:Has[T::Item]]() -> Unit = ()";
    let mut observed = Vec::new();
    for reversed in [false, true] {
        with_resolver(&[text], |resolver, _| {
            let mut ids = resolver
                .collection
                .modules()
                .flat_map(|module| module.declarations())
                .filter(|declaration| declaration.kind() == DeclarationKind::GenericParameter)
                .map(Declaration::id)
                .collect::<Vec<_>>();
            if reversed {
                ids.reverse();
            }
            resolver.prime_source_generic_bounds(&ids).unwrap();
            resolver.resolve_source_generics().unwrap();
            let store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
            observed.push(store.finalize(None).diagnostics().to_vec());
        });
    }
    assert_eq!(observed[0], observed[1]);
    assert_eq!(observed[0].len(), 1);
    assert_eq!(
        observed[0][0]
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range()
            .start()
            .as_usize(),
        text.find("U::Item").unwrap()
    );
}

#[test]
fn a_dependent_cycle_retains_both_its_own_and_the_upstream_cycle_roots() {
    let text = "trait Has[A] { type Item }\nfn bad[T:Has[(T::Item,U::Item)],U:Has[U::Item]]() -> Unit = ()";
    with_resolver(&[text], |resolver, sources| {
        let parameter = resolver
            .collection
            .modules()
            .flat_map(|module| module.declarations())
            .find(|declaration| {
                declaration.kind() == DeclarationKind::GenericParameter
                    && declaration.name().is_some_and(|name| name.text() == "T")
            })
            .unwrap()
            .id();
        let owner = resolver
            .collection
            .declaration(parameter)
            .unwrap()
            .parent()
            .unwrap();
        let span = sources[0]
            .origin_span(resolver.collection.declaration(owner).unwrap().origin())
            .unwrap();
        let checkpoint = resolver.diagnostics.checkpoint();
        resolver.resolve_source_generics().unwrap();
        let FactState::Poisoned(poison) = resolver
            .generic_parameters
            .state(&GenericParameterId::Source(parameter))
        else {
            panic!("cyclic parameter");
        };
        let poison = poison.clone();
        let expected = resolver
            .diagnostics
            .roots_since_within(&checkpoint, span)
            .unwrap()
            .unwrap();
        resolver
            .diagnostics
            .select_preceding_roots(&poison, &expected)
            .unwrap();
        let store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
        let snapshot = store.finalize(None);
        assert_eq!(snapshot.diagnostics().len(), 2);
        assert!(
            snapshot
                .diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.code().as_str() == "GENERIC_BOUND_REFERENCE_CYCLE")
        );
    });
}

#[test]
fn symbolic_projection_identity_contains_the_complete_declaring_reference() {
    let text = "trait Has[A] { type Item }\ntrait Other[A] { type Item }\ntrait Uses[A] {}\nfn valid[T:Uses[T::Item]+Has[U::Item],U:Other[i64]]() -> Unit = ()";
    with_resolver(&[text], |resolver, _| {
        resolver.resolve_source_generics().unwrap();
        assert!(!resolver.diagnostics.has_errors());
        let named = |name| {
            resolver
                .collection
                .modules()
                .flat_map(|module| module.declarations())
                .find(|declaration| declaration.name().is_some_and(|found| found.text() == name))
                .unwrap()
                .id()
        };
        let parameter = GenericParameterId::Source(named("T"));
        let FactState::Known(signature) = resolver.generic_parameters.state(&parameter) else {
            panic!("known parameter");
        };
        let uses = signature
            .bounds()
            .iter()
            .find(|bound| *bound.declaration() == CanonicalTarget::Source(named("Uses")))
            .unwrap();
        let declaring = signature
            .bounds()
            .iter()
            .find(|bound| *bound.declaration() == CanonicalTarget::Source(named("Has")))
            .unwrap();
        let projected = resolver.types.substitution(uses.substitution()).unwrap()[0];
        let TypeKind::AssociatedProjection(projection) = resolver.types.kind(projected).unwrap()
        else {
            panic!("symbolic projection");
        };
        assert_eq!(projection.base(), resolver.generic_types[&parameter]);
        assert_eq!(projection.trait_ref(), declaring);
        let argument = resolver
            .types
            .substitution(declaring.substitution())
            .unwrap()[0];
        let TypeKind::AssociatedProjection(argument) = resolver.types.kind(argument).unwrap()
        else {
            panic!("forward projected argument");
        };
        assert_eq!(
            argument.base(),
            resolver.generic_types[&GenericParameterId::Source(named("U"))]
        );
    });
}
