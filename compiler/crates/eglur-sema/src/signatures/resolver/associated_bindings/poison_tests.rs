use std::cell::Cell;

use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticSnapshot, PrimarySpan, Severity,
};

use super::super::tests::with_resolver;
use super::*;

#[derive(Clone, Copy)]
pub(super) enum Stage {
    Structural,
    Values,
}

pub(super) enum Step {
    Input(Stage),
    Complete(Stage),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    inputs: usize,
    completed: usize,
}

thread_local! {
    static WORK: Cell<[Work; 2]> = const { Cell::new([Work { inputs: 0, completed: 0 }; 2]) };
}

pub(super) fn observe(step: Step) {
    WORK.update(|mut work| {
        match step {
            Step::Input(stage) => work[stage as usize].inputs += 1,
            Step::Complete(stage) => work[stage as usize].completed += 1,
        }
        work
    });
}

fn with_bindings(
    text: &str,
    inspect: impl for<'input, 'hir> FnOnce(
        &mut SignatureResolver<'input, 'hir>,
        &'input SemanticSource<'hir>,
        DefId,
        &'hir eglur_hir::ImplDecl,
        &TraitImplHeader,
    ),
) {
    with_resolver(&[text], |resolver, sources| {
        let source = &sources[0];
        let owner = resolver
            .collection
            .modules()
            .next()
            .unwrap()
            .declarations()
            .find(|declaration| declaration.kind() == DeclarationKind::Impl)
            .unwrap()
            .id();
        let declaration = resolver.collection.declaration(owner).unwrap();
        let ItemKind::Impl(value) = source
            .hir()
            .unwrap()
            .item(declaration.item().unwrap())
            .kind()
        else {
            panic!("fixture contains an impl");
        };
        resolver.prime_source_impl_headers().unwrap();
        let header = resolver.impl_headers.get(&owner).unwrap().clone();
        WORK.set([Work::default(); 2]);
        inspect(resolver, source, owner, value, &header);
    });
}

fn resolve<'hir>(
    resolver: &mut SignatureResolver<'_, 'hir>,
    source: &SemanticSource<'hir>,
    owner: DefId,
    value: &eglur_hir::ImplDecl,
    header: &TraitImplHeader,
) -> Result<Resolution<Vec<AssociatedBindingSignature>>, SignatureFlow> {
    resolver.resolve_source_impl_bindings(
        owner,
        source,
        source.hir().unwrap(),
        value,
        header.trait_ref(),
        header.target(),
    )
}

fn binding_occurrences(
    resolver: &SignatureResolver<'_, '_>,
    source: &SemanticSource<'_>,
    owner: DefId,
    value: &eglur_hir::ImplDecl,
) -> Vec<(CanonicalTarget, TypeOccurrenceId)> {
    let module = resolver.collection.module(owner.module()).unwrap();
    value
        .items
        .iter()
        .filter_map(|&item| {
            let ItemKind::AssociatedTypeBinding(binding) = source.hir().unwrap().item(item).kind()
            else {
                return None;
            };
            Some((
                CanonicalTarget::Source(module.item_def(item).unwrap()),
                TypeOccurrenceId::new(source.file(), binding.value),
            ))
        })
        .collect()
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
    all: &Poison,
    expected: &BTreeMap<String, usize>,
) {
    store
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    store.select_preceding_roots(&poison, all).unwrap();
    let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(None) else {
        panic!("nonempty binding provenance retains its roots");
    };
    let mut found = BTreeMap::new();
    for diagnostic in snapshot.diagnostics() {
        *found
            .entry(diagnostic.code().as_str().to_owned())
            .or_insert(0) += 1;
    }
    assert_eq!(&found, expected);
}

#[test]
fn structural_failures_complete_once_before_supplied_rhs() {
    for count in [8, 32, 128] {
        for mode in ["missing", "unknown", "duplicate"] {
            let required = if mode == "duplicate" {
                "type Item\n".into()
            } else {
                (0..count)
                    .map(|i| format!("type A{i}\n"))
                    .collect::<String>()
            };
            let supplied = match mode {
                "missing" => String::new(),
                "unknown" => (0..count)
                    .map(|i| format!("type B{i} = Missing{i}\n"))
                    .collect(),
                "duplicate" => (0..count).map(|_| "type Item = Missing\n").collect(),
                _ => unreachable!(),
            };
            let text = format!(
                "trait Mark {{ {required} type Keep }}\nstruct Host {{}}\nimpl Mark for Host {{ {supplied} type Keep = MissingEnd }}\n"
            );
            with_bindings(&text, |resolver, source, owner, value, header| {
                let span = source
                    .origin_span(resolver.collection.declaration(owner).unwrap().origin())
                    .unwrap();
                let checkpoint = resolver.diagnostics.checkpoint();
                let Resolution::Poisoned(poison) =
                    resolve(resolver, source, owner, value, header).unwrap()
                else {
                    panic!("structural failures poison the impl");
                };
                let expected = match mode {
                    "missing" => count,
                    "unknown" => 2 * count,
                    "duplicate" => count - 1,
                    _ => unreachable!(),
                };
                assert_eq!(
                    WORK.get(),
                    [
                        Work {
                            inputs: expected,
                            completed: 1
                        },
                        Work::default()
                    ]
                );
                for (binding, occurrence) in binding_occurrences(resolver, source, owner, value) {
                    assert!(matches!(
                        resolver.declarations.state(&binding),
                        FactState::Poisoned(_)
                    ));
                    assert!(matches!(
                        resolver.occurrences.state(&occurrence),
                        FactState::Unperformed
                    ));
                }
                let all = resolver
                    .diagnostics
                    .roots_since_within(&checkpoint, span)
                    .unwrap()
                    .unwrap();
                assert_selected_roots(
                    std::mem::take(resolver.diagnostics),
                    poison,
                    &all,
                    &BTreeMap::from([("TRAIT_IMPL_ASSOC_TYPE_MISMATCH".into(), expected)]),
                );
            });
        }
    }
}

#[test]
fn value_failures_complete_once_and_publish_later_binding() {
    for count in [8, 32, 128] {
        let required = (0..count)
            .map(|i| format!("type A{i}\n"))
            .collect::<String>();
        let supplied = (0..count)
            .map(|i| format!("type A{i} = Missing{i}\n"))
            .collect::<String>();
        let text = format!(
            "trait Mark {{ {required} type End }}\nstruct Host {{}}\nimpl Mark for Host {{ {supplied} type End = Bool }}\n"
        );
        with_bindings(&text, |resolver, source, owner, value, header| {
            let checkpoint = resolver.diagnostics.checkpoint();
            let span = source
                .origin_span(resolver.collection.declaration(owner).unwrap().origin())
                .unwrap();
            let Resolution::Poisoned(poison) =
                resolve(resolver, source, owner, value, header).unwrap()
            else {
                panic!("missing RHS types poison bindings");
            };
            assert_eq!(
                WORK.get(),
                [
                    Work::default(),
                    Work {
                        inputs: count,
                        completed: 1
                    }
                ]
            );
            let occurrences = binding_occurrences(resolver, source, owner, value);
            for (binding, occurrence) in &occurrences[..count] {
                assert!(matches!(
                    resolver.declarations.state(binding),
                    FactState::Poisoned(_)
                ));
                assert!(matches!(
                    resolver.occurrences.state(occurrence),
                    FactState::Poisoned(_)
                ));
            }
            assert!(matches!(
                resolver.declarations.state(&occurrences[count].0),
                FactState::Known(_)
            ));
            assert!(matches!(
                resolver.occurrences.state(&occurrences[count].1),
                FactState::Known(_)
            ));
            let all = resolver
                .diagnostics
                .roots_since_within(&checkpoint, span)
                .unwrap()
                .unwrap();
            assert_selected_roots(
                std::mem::take(resolver.diagnostics),
                poison,
                &all,
                &BTreeMap::from([("NAME_NOT_FOUND".into(), count)]),
            );
        });
    }
}

#[test]
fn cycle_affected_bindings_precede_late_structure_failures() {
    for count in [8, 32, 128] {
        for mode in ["reference", "cycle-reference", "mixed-cycle-late"] {
            let required = (0..count)
                .map(|i| format!("type A{i}\n"))
                .collect::<String>();
            let supplied = (0..count)
                .map(|i| {
                    let ty = match mode {
                        "reference" => "&i64".into(),
                        "cycle-reference" => format!("&Self::A{}", (i + 1) % count),
                        "mixed-cycle-late" if i < count / 2 => {
                            format!("Self::A{}", (i + 1) % (count / 2))
                        }
                        "mixed-cycle-late" => "&i64".into(),
                        _ => unreachable!(),
                    };
                    format!("type A{i} = {ty}\n")
                })
                .collect::<String>();
            let text = format!(
                "trait Mark {{ {required} }}\nstruct Host {{}}\nimpl Mark for Host {{ {supplied} }}\n"
            );
            with_bindings(&text, |resolver, source, owner, value, header| {
                let checkpoint = resolver.diagnostics.checkpoint();
                let span = source
                    .origin_span(resolver.collection.declaration(owner).unwrap().origin())
                    .unwrap();
                let Resolution::Poisoned(poison) =
                    resolve(resolver, source, owner, value, header).unwrap()
                else {
                    panic!("cycle or late structure failure poisons bindings");
                };
                let mut expected = BTreeMap::new();
                if mode != "reference" {
                    expected.insert("ASSOCIATED_TYPE_CYCLE".into(), 1);
                }
                if mode != "cycle-reference" {
                    expected.insert(
                        "REFERENCE_TYPE_POSITION_FORBIDDEN".into(),
                        if mode == "reference" {
                            count
                        } else {
                            count / 2
                        },
                    );
                }
                assert_eq!(
                    WORK.get(),
                    [
                        Work::default(),
                        Work {
                            inputs: expected.values().sum(),
                            completed: 1
                        }
                    ]
                );
                for (binding, occurrence) in binding_occurrences(resolver, source, owner, value) {
                    assert!(matches!(
                        resolver.declarations.state(&binding),
                        FactState::Poisoned(_)
                    ));
                    assert!(matches!(
                        resolver.occurrences.state(&occurrence),
                        FactState::Known(_)
                    ));
                }
                let all = resolver
                    .diagnostics
                    .roots_since_within(&checkpoint, span)
                    .unwrap()
                    .unwrap();
                assert_selected_roots(
                    std::mem::take(resolver.diagnostics),
                    poison,
                    &all,
                    &expected,
                );
            });
        }
    }
}

#[test]
fn seeded_value_provenance_keeps_partial_distinct_repeated_and_overlapping_roots() {
    for count in [8, 32, 128] {
        for mode in [
            "known",
            "distinct",
            "repeat",
            "overlap",
            "partial",
            "partial-first-poison",
            "partial-last-poison",
        ] {
            let aliases = (0..count)
                .map(|i| format!("type X{i} = i64\n"))
                .collect::<String>();
            let required = (0..count)
                .map(|i| format!("type A{i}\n"))
                .collect::<String>();
            let supplied = (0..count)
                .map(|i| format!("type A{i} = X{i}\n"))
                .collect::<String>();
            let text = format!(
                "{aliases}trait Mark {{ {required} type End }}\nstruct Host {{}}\nimpl Mark for Host {{ {supplied} type End = Bool }}\n"
            );
            with_bindings(&text, |resolver, source, owner, value, header| {
                let definitions = resolver
                    .collection
                    .modules()
                    .next()
                    .unwrap()
                    .declarations()
                    .filter_map(|d| d.name().map(|name| (name.text().to_owned(), d.id())))
                    .collect::<BTreeMap<_, _>>();
                let inputs = roots(resolver.diagnostics, count);
                let mut union = PoisonAccumulator::new();
                for input in &inputs {
                    union.include(input).unwrap();
                }
                let all = union.finish().unwrap();
                for i in 0..count {
                    let poison = match mode {
                        "distinct" => Some(inputs[i].clone()),
                        "repeat" => Some(inputs[0].clone()),
                        "overlap" => Some(inputs[i].merge(&inputs[(i + 1) % count]).unwrap()),
                        "partial-first-poison" if i == 0 => Some(inputs[0].clone()),
                        "partial-last-poison" if i + 1 == count => Some(inputs[0].clone()),
                        _ => None,
                    };
                    if let Some(poison) = poison {
                        resolver
                            .alias_states
                            .insert(definitions[&format!("X{i}")], AliasState::Poisoned(poison));
                    } else if mode != "known" {
                        resolver
                            .alias_states
                            .insert(definitions[&format!("X{i}")], AliasState::Unperformed);
                    }
                }
                let result = resolve(resolver, source, owner, value, header).unwrap();
                let included = match mode {
                    "distinct" | "repeat" | "overlap" => count,
                    "partial-first-poison" | "partial-last-poison" => 1,
                    _ => 0,
                };
                assert_eq!(
                    WORK.get(),
                    [
                        Work::default(),
                        Work {
                            inputs: included,
                            completed: usize::from(included != 0)
                        }
                    ]
                );
                let occurrences = binding_occurrences(resolver, source, owner, value);
                assert!(matches!(
                    resolver.declarations.state(&occurrences[count].0),
                    FactState::Known(_)
                ));
                assert!(matches!(
                    resolver.occurrences.state(&occurrences[count].1),
                    FactState::Known(_)
                ));
                match result {
                    Resolution::Known(bindings) => {
                        assert_eq!(mode, "known");
                        assert_eq!(bindings.len(), count + 1);
                    }
                    Resolution::Unperformed => assert_eq!(mode, "partial"),
                    Resolution::Poisoned(poison) => {
                        let expected = if mode == "distinct" || mode == "overlap" {
                            (0..count).map(|i| (format!("ROOT_{i:03}"), 1)).collect()
                        } else {
                            BTreeMap::from([("ROOT_000".into(), 1)])
                        };
                        assert_selected_roots(
                            std::mem::take(resolver.diagnostics),
                            poison,
                            &all,
                            &expected,
                        );
                    }
                }
            });
        }
    }
}

#[test]
fn foreign_value_error_preserves_published_prefix_and_skips_completion() {
    for foreign_first in [false, true] {
        with_bindings(
            "type X0 = i64\ntype X1 = i64\ntrait Mark { type A0\ntype A1\ntype End }\nstruct Host {}\nimpl Mark for Host { type A0 = X0\ntype A1 = X1\ntype End = Bool }\n",
            |resolver, source, owner, value, header| {
                let definitions = resolver
                    .collection
                    .modules()
                    .next()
                    .unwrap()
                    .declarations()
                    .filter_map(|d| d.name().map(|name| (name.text().to_owned(), d.id())))
                    .collect::<BTreeMap<_, _>>();
                let local = roots(resolver.diagnostics, 1);
                let mut foreign_store = DiagnosticStore::new();
                let foreign = roots(&mut foreign_store, 1);
                for (i, is_foreign) in [(0, foreign_first), (1, !foreign_first)] {
                    resolver.alias_states.insert(
                        definitions[&format!("X{i}")],
                        AliasState::Poisoned(if is_foreign {
                            foreign[0].clone()
                        } else {
                            local[0].clone()
                        }),
                    );
                }
                let Err(error) = resolve(resolver, source, owner, value, header) else {
                    panic!("second pool is rejected");
                };
                assert_eq!(
                    error,
                    SignatureFlow::Internal(SignatureResolutionError::DiagnosticStore(
                        DiagnosticStoreError::ForeignPoison
                    ))
                );
                assert_eq!(
                    WORK.get(),
                    [
                        Work::default(),
                        Work {
                            inputs: 2,
                            completed: 0
                        }
                    ]
                );
                let occurrences = binding_occurrences(resolver, source, owner, value);
                for (i, is_foreign) in [(0, foreign_first), (1, !foreign_first)] {
                    let FactState::Poisoned(declaration_poison) =
                        resolver.declarations.state(&occurrences[i].0)
                    else {
                        panic!("failed declaration is published before refusal");
                    };
                    let FactState::Poisoned(occurrence_poison) =
                        resolver.occurrences.state(&occurrences[i].1)
                    else {
                        panic!("failed occurrence is published before refusal");
                    };
                    for poison in [declaration_poison.clone(), occurrence_poison.clone()] {
                        if is_foreign {
                            foreign_store
                                .record_failure(DiagnosticFailure::Derivative(poison.clone()))
                                .unwrap();
                            assert_eq!(
                                resolver
                                    .diagnostics
                                    .record_failure(DiagnosticFailure::Derivative(poison))
                                    .unwrap_err(),
                                DiagnosticStoreError::ForeignPoison
                            );
                        } else {
                            resolver
                                .diagnostics
                                .record_failure(DiagnosticFailure::Derivative(poison.clone()))
                                .unwrap();
                            assert_eq!(
                                foreign_store
                                    .record_failure(DiagnosticFailure::Derivative(poison))
                                    .unwrap_err(),
                                DiagnosticStoreError::ForeignPoison
                            );
                        }
                    }
                }
                assert!(matches!(
                    resolver.declarations.state(&occurrences[2].0),
                    FactState::Unperformed
                ));
                assert!(matches!(
                    resolver.occurrences.state(&occurrences[2].1),
                    FactState::Unperformed
                ));
            },
        );
    }
}
