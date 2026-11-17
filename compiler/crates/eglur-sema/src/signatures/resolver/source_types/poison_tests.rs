use std::cell::Cell;

use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticSnapshot, PrimarySpan, Severity,
};

use super::super::tests::with_resolver;
use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Work {
    pub(super) inputs: usize,
    pub(super) completed: usize,
}

pub(super) enum Step {
    Input,
    Complete,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { inputs: 0, completed: 0 }) };
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

pub(super) fn roots(store: &mut DiagnosticStore, count: usize) -> Vec<Poison> {
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

pub(super) fn aggregate(inputs: &[Poison], ids: impl IntoIterator<Item = usize>) -> Poison {
    let mut pending = PoisonAccumulator::new();
    for index in ids {
        pending.include(&inputs[index]).unwrap();
    }
    pending.finish().unwrap()
}

pub(super) fn assert_roots(
    mut store: DiagnosticStore,
    inputs: &[Poison],
    poison: &Poison,
    expected: impl IntoIterator<Item = usize>,
) {
    store
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    store
        .select_preceding_roots(poison, &aggregate(inputs, 0..inputs.len()))
        .unwrap();
    let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(None) else {
        panic!("nonempty provenance retains its roots");
    };
    assert_eq!(
        snapshot
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code().as_str())
            .collect::<Vec<_>>(),
        expected
            .into_iter()
            .map(|index| format!("ROOT_{index:03}"))
            .collect::<Vec<_>>()
    );
}

fn list(
    resolver: &mut SignatureResolver<'_, '_>,
    source: &SemanticSource<'_>,
) -> (DefId, Vec<TypeRefId>, SourceTypeContext) {
    let declaration = resolver
        .collection
        .modules()
        .next()
        .unwrap()
        .declarations()
        .find(|declaration| declaration.name().is_some_and(|name| name.text() == "Seed"))
        .unwrap();
    let owner = declaration.id();
    let hir = source.hir().unwrap();
    let ItemKind::TypeAlias(alias) = hir.item(declaration.item().unwrap()).kind() else {
        panic!("Seed is a type alias");
    };
    let TypeRefKind::Tuple(elements) = hir.type_ref(alias.value).kind() else {
        panic!("Seed has tuple elements");
    };
    (
        owner,
        elements.to_vec(),
        resolver.source_context(owner, false).unwrap(),
    )
}

#[test]
fn source_type_lists_complete_distinct_roots_once() {
    for count in [8, 32, 128] {
        let names = (0..count)
            .map(|index| format!("Missing{index}"))
            .collect::<Vec<_>>();
        let text = format!("type Seed = ({})\n", names.join(","));
        with_resolver(&[&text], |resolver, sources| {
            let source = &sources[0];
            let (owner, elements, context) = list(resolver, source);
            WORK.set(Work::default());

            let result = resolver
                .resolve_source_types(
                    owner,
                    source,
                    source.hir().unwrap(),
                    &elements,
                    TypePosition::Ordinary,
                    &context,
                )
                .unwrap();
            assert!(matches!(result, Resolution::Poisoned(_)));
            assert_eq!(
                WORK.get(),
                Work {
                    inputs: count,
                    completed: 1
                }
            );
            assert!(elements.iter().all(|&element| matches!(
                resolver.occurrences.state(&TypeOccurrenceId::new(source.file(), element)),
                FactState::Poisoned(_)
            )));

            let store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
            let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(None) else {
                panic!("all missing types remain diagnosed");
            };
            assert_eq!(snapshot.diagnostics().len(), count);
            assert_eq!(
                snapshot
                    .diagnostics()
                    .iter()
                    .map(|diagnostic| {
                        assert_eq!(diagnostic.code().as_str(), "NAME_NOT_FOUND");
                        diagnostic.data().get("name").unwrap().clone()
                    })
                    .collect::<Vec<_>>(),
                names
                    .iter()
                    .map(|name| DiagnosticDataValue::from(name.as_str()))
                    .collect::<Vec<_>>()
            );
        });
    }
}

#[test]
fn known_and_empty_source_type_lists_do_not_complete_provenance() {
    with_resolver(&["type Seed = (i64, Bool)\n"], |resolver, sources| {
        let source = &sources[0];
        let (owner, elements, context) = list(resolver, source);
        WORK.set(Work::default());
        let Resolution::Known(values) = resolver
            .resolve_source_types(
                owner,
                source,
                source.hir().unwrap(),
                &elements,
                TypePosition::Ordinary,
                &context,
            )
            .unwrap()
        else {
            panic!("known source types");
        };
        assert_eq!(values.len(), 2);
        assert!(matches!(
            resolver.types.kind(values[0]).unwrap(),
            TypeKind::Primitive(crate::PrimitiveType::I64)
        ));
        assert!(matches!(
            resolver.types.kind(values[1]).unwrap(),
            TypeKind::Primitive(crate::PrimitiveType::Bool)
        ));
        let Resolution::Known(empty) = resolver
            .resolve_source_types(
                owner,
                source,
                source.hir().unwrap(),
                &[],
                TypePosition::Ordinary,
                &context,
            )
            .unwrap()
        else {
            panic!("empty source types");
        };
        assert!(empty.is_empty());
        assert_eq!(WORK.get(), Work::default());
    });
}

#[test]
fn source_type_positions_zip_without_visiting_extra_types_or_positions() {
    for limit in [2, 6] {
        with_resolver(
            &["type Seed = (Missing0, i64, Missing2, Bool)\n"],
            |resolver, sources| {
                let source = &sources[0];
                let (owner, elements, context) = list(resolver, source);
                let visits = Cell::new(0);
                let positions = std::iter::repeat_n(TypePosition::Ordinary, limit).inspect(|_| {
                    visits.set(visits.get() + 1);
                });
                WORK.set(Work::default());
                let result = resolver
                    .resolve_source_types_with_positions(
                        owner,
                        source,
                        source.hir().unwrap(),
                        &elements,
                        positions,
                        &context,
                    )
                    .unwrap();
                assert!(matches!(result, Resolution::Poisoned(_)));
                let count = limit.min(elements.len());
                assert_eq!(visits.get(), count);
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: count / 2,
                        completed: 1
                    }
                );
                for &element in &elements[count..] {
                    assert!(matches!(
                        resolver
                            .occurrences
                            .state(&TypeOccurrenceId::new(source.file(), element)),
                        FactState::Unperformed
                    ));
                }
            },
        );
    }
}

#[test]
fn source_type_lists_preserve_partial_state_and_poison_precedence() {
    for poisoned in [None, Some("A"), Some("B")] {
        with_resolver(
            &["type A = i64\ntype B = Bool\ntype Seed = (A, B, i64)\n"],
            |resolver, sources| {
                let source = &sources[0];
                let (owner, elements, context) = list(resolver, source);
                let inputs = roots(resolver.diagnostics, 1);
                let aliases = resolver
                    .collection
                    .modules()
                    .next()
                    .unwrap()
                    .declarations()
                    .filter_map(|declaration| {
                        declaration
                            .name()
                            .map(|name| (name.text().to_owned(), declaration.id()))
                    })
                    .collect::<BTreeMap<_, _>>();
                for name in ["A", "B"] {
                    let state = if poisoned == Some(name) {
                        AliasState::Poisoned(inputs[0].clone())
                    } else {
                        AliasState::Unperformed
                    };
                    resolver.alias_states.insert(aliases[name], state);
                }
                WORK.set(Work::default());

                let result = resolver
                    .resolve_source_types(
                        owner,
                        source,
                        source.hir().unwrap(),
                        &elements,
                        TypePosition::Ordinary,
                        &context,
                    )
                    .unwrap();
                assert!(matches!(
                    resolver
                        .occurrences
                        .state(&TypeOccurrenceId::new(source.file(), elements[2])),
                    FactState::Known(_)
                ));
                for (index, name) in ["A", "B"].into_iter().enumerate() {
                    let state = resolver
                        .occurrences
                        .state(&TypeOccurrenceId::new(source.file(), elements[index]));
                    if poisoned == Some(name) {
                        assert!(matches!(state, FactState::Poisoned(_)));
                    } else {
                        assert!(matches!(state, FactState::Unperformed));
                    }
                }
                if poisoned.is_some() {
                    assert!(matches!(&result, Resolution::Poisoned(_)));
                    let Resolution::Poisoned(poison) = result else {
                        panic!("poison takes precedence");
                    };
                    assert_eq!(
                        WORK.get(),
                        Work {
                            inputs: 1,
                            completed: 1
                        }
                    );
                    let store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
                    assert_roots(store, &inputs, &poison, [0]);
                } else {
                    assert!(matches!(result, Resolution::Unperformed));
                    assert_eq!(WORK.get(), Work::default());
                }
            },
        );
    }
}

#[test]
fn source_type_list_foreign_error_preserves_prior_occurrences_and_stops_visiting() {
    with_resolver(
        &["type A = i64\ntype B = Bool\ntype C = Str\ntype Seed = (A, B, C)\n"],
        |resolver, sources| {
            let source = &sources[0];
            let (owner, elements, context) = list(resolver, source);
            let own = roots(resolver.diagnostics, 1);
            let mut foreign_store = DiagnosticStore::new();
            let foreign = roots(&mut foreign_store, 1);
            let aliases = resolver
                .collection
                .modules()
                .next()
                .unwrap()
                .declarations()
                .filter_map(|declaration| {
                    declaration
                        .name()
                        .map(|name| (name.text().to_owned(), declaration.id()))
                })
                .collect::<BTreeMap<_, _>>();
            resolver
                .alias_states
                .insert(aliases["A"], AliasState::Poisoned(own[0].clone()));
            resolver
                .alias_states
                .insert(aliases["B"], AliasState::Poisoned(foreign[0].clone()));
            WORK.set(Work::default());

            let Err(error) = resolver.resolve_source_types(
                owner,
                source,
                source.hir().unwrap(),
                &elements,
                TypePosition::Ordinary,
                &context,
            ) else {
                panic!("foreign provenance is rejected");
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
            for (index, store) in [(0, &mut *resolver.diagnostics), (1, &mut foreign_store)] {
                let FactState::Poisoned(poison) = resolver
                    .occurrences
                    .state(&TypeOccurrenceId::new(source.file(), elements[index]))
                else {
                    panic!("prior occurrence is retained");
                };
                store
                    .record_failure(DiagnosticFailure::Derivative(poison.clone()))
                    .unwrap();
            }
            assert!(matches!(
                resolver
                    .occurrences
                    .state(&TypeOccurrenceId::new(source.file(), elements[2])),
                FactState::Unperformed
            ));
        },
    );
}
