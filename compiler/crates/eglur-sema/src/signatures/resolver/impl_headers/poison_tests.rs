use std::cell::Cell;

use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticSnapshot, PrimarySpan, Severity,
};

use super::super::tests::with_resolver;
use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    inputs: usize,
    completed: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { inputs: 0, completed: 0 }) };
}

pub(super) enum Step {
    Input,
    Complete,
}

pub(super) fn observe(step: Step) {
    WORK.update(|mut work| {
        match step {
            Step::Input => work.inputs += 1,
            Step::Complete => work.completed += 1,
        }
        work
    });
}

fn with_parameter(
    text: &str,
    inspect: impl for<'input, 'hir> FnOnce(
        &mut SignatureResolver<'input, 'hir>,
        &'input SemanticSource<'hir>,
        DefId,
    ),
) {
    with_resolver(&[text], |resolver, sources| {
        let id = resolver
            .collection
            .modules()
            .next()
            .unwrap()
            .declarations()
            .find(|declaration| {
                declaration.kind() == DeclarationKind::GenericParameter
                    && declaration.name().is_some_and(|name| name.text() == "T")
            })
            .unwrap()
            .id();
        WORK.set(Work::default());
        inspect(resolver, &sources[0], id);
    });
}

fn roots(store: &mut DiagnosticStore, count: usize) -> Vec<Poison> {
    (0..count)
        .map(|index| {
            let diagnostic = Diagnostic::new(
                DiagnosticCode::new(format!("ROOT_{index:03}")).unwrap(),
                Severity::Error,
                DiagnosticPhase::new("type").unwrap(),
                "independent failure",
                PrimarySpan::Unavailable,
            );
            let root = DiagnosticRoot::boundary(
                BoundaryRootKey::new(format!("root-{index}")).unwrap(),
                diagnostic,
            )
            .unwrap();
            store.record_failure(DiagnosticFailure::Root(root)).unwrap()
        })
        .collect()
}

fn assert_selected_roots(
    mut store: DiagnosticStore,
    poison: Poison,
    expected: &Poison,
    codes: &[(&str, usize)],
) {
    store
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    store.select_preceding_roots(&poison, expected).unwrap();
    let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(None) else {
        panic!("nonempty bound provenance retains its roots");
    };
    let mut actual = BTreeMap::new();
    for diagnostic in snapshot.diagnostics() {
        *actual.entry(diagnostic.code().as_str()).or_insert(0) += 1;
    }
    assert_eq!(actual, codes.iter().copied().collect());
}

#[test]
fn generic_failed_and_duplicate_bounds_complete_once() {
    for count in [8, 32, 128] {
        for mode in ["unknown", "duplicate", "mixed"] {
            let traits = (0..count)
                .map(|index| format!("trait Mark{index} {{}}\n"))
                .collect::<String>();
            let bounds = (0..count)
                .map(|index| match mode {
                    "unknown" => format!("Missing{index}"),
                    "duplicate" => "Mark0".to_owned(),
                    "mixed" if index % 2 == 0 => format!("Mark{index}"),
                    "mixed" => format!("Missing{index}"),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>()
                .join("+");
            let text = format!("{traits}fn drive[T:{bounds}]() -> Unit = ()\n");
            with_parameter(&text, |resolver, source, id| {
                let owner = resolver
                    .collection
                    .declaration(id)
                    .unwrap()
                    .parent()
                    .unwrap();
                let span = source
                    .origin_span(resolver.collection.declaration(owner).unwrap().origin())
                    .unwrap();
                let checkpoint = resolver.diagnostics.checkpoint();
                let Resolution::Poisoned(poison) = resolver.resolve_source_generic(id).unwrap()
                else {
                    panic!("invalid bounds poison the parameter");
                };
                let inputs = match mode {
                    "duplicate" => count - 1,
                    "mixed" => count / 2,
                    _ => count,
                };
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs,
                        completed: 1
                    }
                );
                assert!(resolver.source_generic_stack.is_empty());
                assert!(
                    !resolver
                        .generic_bounds
                        .contains_key(&GenericParameterId::Source(id))
                );
                assert!(matches!(
                    resolver
                        .generic_parameters
                        .state(&GenericParameterId::Source(id)),
                    FactState::Poisoned(_)
                ));
                assert!(matches!(
                    resolver.declarations.state(&CanonicalTarget::Source(id)),
                    FactState::Poisoned(_)
                ));
                let before = WORK.get();
                assert!(matches!(
                    resolver.resolve_source_generic(id).unwrap(),
                    Resolution::Poisoned(_)
                ));
                assert_eq!(WORK.get(), before);
                let expected = resolver
                    .diagnostics
                    .roots_since_within(&checkpoint, span)
                    .unwrap()
                    .unwrap();
                let store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
                let code = if mode == "duplicate" {
                    "TRAIT_BOUND_INVALID"
                } else {
                    "NAME_NOT_FOUND"
                };
                assert_selected_roots(store, poison, &expected, &[(code, inputs)]);
            });
        }
    }
}

#[test]
fn known_bounds_preserve_declaration_and_canonical_orders_without_completion() {
    for count in [0, 8, 32, 128] {
        let traits = (0..count)
            .map(|index| format!("trait Mark{index} {{}}\n"))
            .collect::<String>();
        let bounds = (0..count)
            .rev()
            .map(|index| format!("Mark{index}"))
            .collect::<Vec<_>>()
            .join("+");
        let parameter = if count == 0 {
            "T".to_owned()
        } else {
            format!("T:{bounds}")
        };
        let text = format!("{traits}fn drive[{parameter}]() -> Unit = ()\n");
        with_parameter(&text, |resolver, source, id| {
            assert!(matches!(
                resolver.resolve_source_generic(id).unwrap(),
                Resolution::Known(())
            ));
            let parameter = resolver.source_generic_parameter(id).unwrap().1;
            let written = parameter
                .bounds
                .iter()
                .map(|&bound| {
                    let FactState::Known(value) = resolver
                        .occurrences
                        .state(&TypeOccurrenceId::new(source.file(), bound))
                    else {
                        panic!("known bound occurrence");
                    };
                    let ResolvedType::Trait(value) = value.resolved() else {
                        panic!("trait bound");
                    };
                    value.clone()
                })
                .collect::<Vec<_>>();
            let FactState::Known(signature) = resolver
                .generic_parameters
                .state(&GenericParameterId::Source(id))
            else {
                panic!("known parameter");
            };
            assert_eq!(signature.declared_bounds(), written);
            let mut canonical = written;
            canonical.sort();
            assert_eq!(signature.bounds(), canonical);
            assert_eq!(
                resolver.generic_bounds[&GenericParameterId::Source(id)]
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>(),
                canonical
            );
            assert!(resolver.source_generic_stack.is_empty());
            assert!(matches!(
                resolver.resolve_source_generic(id).unwrap(),
                Resolution::Known(())
            ));
            assert_eq!(WORK.get(), Work::default());
        });
    }
}

#[test]
fn seeded_bound_roots_preserve_partial_precedence_overlap_and_cache() {
    for count in [8, 32, 128] {
        for mode in [
            "distinct",
            "repeat",
            "overlap",
            "partial",
            "partial-first-poison",
            "partial-last-poison",
        ] {
            let traits = (0..=count)
                .map(|index| format!("trait Mark{index} {{}}\n"))
                .collect::<String>();
            let bounds = (0..=count)
                .map(|index| format!("Mark{index}"))
                .collect::<Vec<_>>()
                .join("+");
            let text = format!("{traits}fn drive[T:{bounds}]() -> Unit = ()\n");
            with_parameter(&text, |resolver, source, id| {
                let inputs = roots(resolver.diagnostics, count);
                let all = inputs
                    .iter()
                    .cloned()
                    .reduce(|left, right| left.merge(&right).unwrap())
                    .unwrap();
                let parameter = resolver.source_generic_parameter(id).unwrap().1.clone();
                let owner = resolver
                    .collection
                    .declaration(id)
                    .unwrap()
                    .parent()
                    .unwrap();
                let context = resolver.source_context(owner, false).unwrap();
                let mut included = 0;
                for (index, &bound) in parameter.bounds[..count].iter().enumerate() {
                    let poison = match mode {
                        "distinct" => Some(inputs[index].clone()),
                        "repeat" => Some(inputs[0].clone()),
                        "overlap" => {
                            Some(inputs[index].merge(&inputs[(index + 1) % count]).unwrap())
                        }
                        "partial-first-poison" if index == 0 => Some(inputs[0].clone()),
                        "partial-last-poison" if index + 1 == count => Some(inputs[0].clone()),
                        _ => None,
                    };
                    let value = if let Some(poison) = poison {
                        included += 1;
                        Resolution::Poisoned(poison)
                    } else {
                        Resolution::Unperformed
                    };
                    resolver
                        .source_generic_bound_seeds
                        .insert((id, bound), value);
                }
                let last = parameter.bounds[count];
                let Resolution::Known(last_value) = resolver
                    .resolve_source_trait_ref(owner, source, source.hir().unwrap(), last, &context)
                    .unwrap()
                else {
                    panic!("later known bound");
                };
                resolver
                    .source_generic_bound_seeds
                    .insert((id, last), Resolution::Known(last_value));
                let result = resolver.resolve_source_generic(id).unwrap();
                assert!(resolver.source_generic_stack.is_empty());
                assert!(
                    !resolver
                        .generic_bounds
                        .contains_key(&GenericParameterId::Source(id))
                );
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: included,
                        completed: usize::from(included != 0)
                    }
                );
                let before = WORK.get();
                let cached = resolver.resolve_source_generic(id).unwrap();
                assert_eq!(WORK.get(), before);
                if included != 0 {
                    let Resolution::Poisoned(poison) = result else {
                        panic!("poison wins over partial bounds");
                    };
                    let Resolution::Poisoned(cached) = cached else {
                        panic!("cached poison");
                    };
                    resolver
                        .diagnostics
                        .record_failure(DiagnosticFailure::Derivative(cached))
                        .unwrap();
                    let store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
                    let codes = if mode == "distinct" || mode == "overlap" {
                        (0..count)
                            .map(|index| (format!("ROOT_{index:03}"), 1))
                            .collect::<Vec<_>>()
                    } else {
                        vec![("ROOT_000".to_owned(), 1)]
                    };
                    assert_selected_roots(
                        store,
                        poison,
                        &all,
                        &codes
                            .iter()
                            .map(|(code, count)| (code.as_str(), *count))
                            .collect::<Vec<_>>(),
                    );
                } else {
                    assert!(matches!(result, Resolution::Unperformed));
                    assert!(matches!(cached, Resolution::Unperformed));
                }
            });
        }
    }
}

#[test]
fn foreign_bound_error_retains_attempt_state_and_unpublished_facts() {
    for foreign_first in [false, true] {
        with_parameter(
            "trait A {}\ntrait B {}\ntrait C {}\nfn drive[T:A+B+C]() -> Unit = ()\n",
            |resolver, _, id| {
                let own = roots(resolver.diagnostics, 1);
                let mut foreign_store = DiagnosticStore::new();
                let foreign = roots(&mut foreign_store, 1);
                let parameter = resolver.source_generic_parameter(id).unwrap().1.clone();
                for (index, foreign_input) in [(0, foreign_first), (1, !foreign_first)] {
                    resolver.source_generic_bound_seeds.insert(
                        (id, parameter.bounds[index]),
                        Resolution::Poisoned(if foreign_input {
                            foreign[0].clone()
                        } else {
                            own[0].clone()
                        }),
                    );
                }
                let Err(error) = resolver.resolve_source_generic(id) else {
                    panic!("foreign bound provenance is rejected");
                };
                assert_eq!(
                    error,
                    SignatureFlow::Internal(SignatureResolutionError::DiagnosticStore(
                        DiagnosticStoreError::ForeignPoison
                    ))
                );
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: 2,
                        completed: 0
                    }
                );
                assert!(matches!(
                    resolver.source_generic_states.get(&id),
                    Some(SourceGenericState::Resolving)
                ));
                assert_eq!(resolver.source_generic_stack, vec![id]);
                assert!(matches!(
                    resolver
                        .generic_parameters
                        .state(&GenericParameterId::Source(id)),
                    FactState::Unperformed
                ));
                assert!(matches!(
                    resolver.declarations.state(&CanonicalTarget::Source(id)),
                    FactState::Unperformed
                ));
                assert!(matches!(
                    resolver.resolve_source_generic(id).unwrap(),
                    Resolution::Unperformed
                ));
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: 2,
                        completed: 0
                    }
                );
                assert_eq!(resolver.source_generic_stack, vec![id]);
            },
        );
    }
}
