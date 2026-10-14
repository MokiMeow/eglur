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

fn definitions(resolver: &SignatureResolver<'_, '_>) -> BTreeMap<String, DefId> {
    resolver
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
        panic!("nonempty field provenance retains its roots");
    };
    let mut actual = BTreeMap::new();
    for diagnostic in snapshot.diagnostics() {
        *actual
            .entry(diagnostic.code().as_str().to_owned())
            .or_insert(0) += 1;
    }
    assert_eq!(&actual, expected);
}

#[test]
fn named_field_failures_complete_once_and_publish_later_fields() {
    for count in [8, 32, 128] {
        let fields = (0..count)
            .map(|index| format!("f{index}:Missing{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let text = format!("enum E {{ Item {{ {fields}, end:Bool }} }}\n");
        with_resolver(&[&text], |resolver, sources| {
            let source = &sources[0];
            let defs = definitions(resolver);
            let owner = defs["Item"];
            let span = source
                .origin_span(resolver.collection.declaration(owner).unwrap().origin())
                .unwrap();
            let checkpoint = resolver.diagnostics.checkpoint();
            WORK.set(Work::default());
            let Resolution::Poisoned(poison) = resolver
                .resolve_source_enum_variant(owner, source, source.hir().unwrap())
                .unwrap()
            else {
                panic!("invalid field types poison the variant");
            };
            assert_eq!(
                WORK.get(),
                Work {
                    inputs: count,
                    completed: 1
                }
            );
            for index in 0..count {
                assert!(matches!(
                    resolver
                        .declarations
                        .state(&CanonicalTarget::Source(defs[&format!("f{index}")])),
                    FactState::Poisoned(_)
                ));
            }
            assert!(matches!(
                resolver
                    .declarations
                    .state(&CanonicalTarget::Source(defs["end"])),
                FactState::Known(_)
            ));
            let all = resolver
                .diagnostics
                .roots_since_within(&checkpoint, span)
                .unwrap()
                .unwrap();
            let store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
            assert_selected_roots(
                store,
                poison,
                &all,
                &BTreeMap::from([("NAME_NOT_FOUND".to_owned(), count)]),
            );
        });
    }
}

#[test]
fn named_seeded_fields_preserve_overlap_partial_and_poison_precedence() {
    for count in [8, 32, 128] {
        for mode in [
            "distinct",
            "repeat",
            "overlap",
            "partial",
            "partial-first-poison",
            "partial-last-poison",
        ] {
            let aliases = (0..count)
                .map(|index| format!("type A{index} = i64\n"))
                .collect::<String>();
            let fields = (0..count)
                .map(|index| format!("f{index}:A{index}"))
                .collect::<Vec<_>>()
                .join(",");
            let text = format!("{aliases}enum E {{ Item {{ {fields}, end:Bool }} }}\n");
            with_resolver(&[&text], |resolver, sources| {
                let source = &sources[0];
                let defs = definitions(resolver);
                let inputs = roots(resolver.diagnostics, count);
                let all = inputs
                    .iter()
                    .cloned()
                    .reduce(|left, right| left.merge(&right).unwrap())
                    .unwrap();
                let mut included = 0;
                for index in 0..count {
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
                    let state = if let Some(poison) = poison {
                        included += 1;
                        AliasState::Poisoned(poison)
                    } else {
                        AliasState::Unperformed
                    };
                    resolver
                        .alias_states
                        .insert(defs[&format!("A{index}")], state);
                }
                WORK.set(Work::default());
                let result = resolver
                    .resolve_source_enum_variant(defs["Item"], source, source.hir().unwrap())
                    .unwrap();
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: included,
                        completed: usize::from(included != 0)
                    }
                );
                assert!(matches!(
                    resolver
                        .declarations
                        .state(&CanonicalTarget::Source(defs["end"])),
                    FactState::Known(_)
                ));
                for index in 0..count {
                    let fact = resolver
                        .declarations
                        .state(&CanonicalTarget::Source(defs[&format!("f{index}")]));
                    let poisoned = matches!(mode, "distinct" | "repeat" | "overlap")
                        || mode == "partial-first-poison" && index == 0
                        || mode == "partial-last-poison" && index + 1 == count;
                    assert_eq!(matches!(fact, FactState::Poisoned(_)), poisoned);
                    assert_eq!(matches!(fact, FactState::Unperformed), !poisoned);
                }
                if included == 0 {
                    assert!(matches!(result, Resolution::Unperformed));
                } else {
                    let Resolution::Poisoned(poison) = result else {
                        panic!("poison dominates partial fields");
                    };
                    let expected = if mode == "distinct" || mode == "overlap" {
                        (0..count)
                            .map(|index| (format!("ROOT_{index:03}"), 1))
                            .collect()
                    } else {
                        BTreeMap::from([("ROOT_000".to_owned(), 1)])
                    };
                    let store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
                    assert_selected_roots(store, poison, &all, &expected);
                }
            });
        }
    }
}

#[test]
fn known_named_fields_preserve_payload_order_without_completion() {
    for count in [0, 8, 32, 128] {
        let fields = (0..count)
            .rev()
            .map(|index| format!("f{index}:i64"))
            .chain(["end:Bool".to_owned()])
            .collect::<Vec<_>>()
            .join(",");
        let text = format!("enum E {{ Item {{ {fields} }} }}\n");
        with_resolver(&[&text], |resolver, sources| {
            let source = &sources[0];
            let defs = definitions(resolver);
            WORK.set(Work::default());
            let Resolution::Known(DeclarationSignature::EnumVariant {
                payload: EnumVariantPayloadSignature::Named(actual),
                ..
            }) = resolver
                .resolve_source_enum_variant(defs["Item"], source, source.hir().unwrap())
                .unwrap()
            else {
                panic!("known named payload");
            };
            let expected = (0..count)
                .rev()
                .map(|index| CanonicalTarget::Source(defs[&format!("f{index}")]))
                .chain([CanonicalTarget::Source(defs["end"])])
                .collect::<Vec<_>>();
            assert_eq!(actual.as_ref(), expected);
            for field in &actual {
                assert!(matches!(
                    resolver.declarations.state(field),
                    FactState::Known(_)
                ));
            }
            assert_eq!(WORK.get(), Work::default());
        });
    }
}

#[test]
fn foreign_named_field_error_preserves_published_prefix() {
    for foreign_first in [false, true] {
        with_resolver(
            &[
                "type A = i64\ntype B = i64\ntype C = i64\nenum E { Item { first:A, second:B, third:C, end:Bool } }\n",
            ],
            |resolver, sources| {
                let source = &sources[0];
                let defs = definitions(resolver);
                let own = roots(resolver.diagnostics, 1);
                let mut foreign_store = DiagnosticStore::new();
                let foreign = roots(&mut foreign_store, 1);
                for (name, is_foreign) in [("A", foreign_first), ("B", !foreign_first)] {
                    resolver.alias_states.insert(
                        defs[name],
                        AliasState::Poisoned(if is_foreign {
                            foreign[0].clone()
                        } else {
                            own[0].clone()
                        }),
                    );
                }
                WORK.set(Work::default());
                let Err(error) = resolver.resolve_source_enum_variant(
                    defs["Item"],
                    source,
                    source.hir().unwrap(),
                ) else {
                    panic!("foreign pool rejected at second field");
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
                for (name, is_foreign) in [("first", foreign_first), ("second", !foreign_first)] {
                    let FactState::Poisoned(poison) = resolver
                        .declarations
                        .state(&CanonicalTarget::Source(defs[name]))
                    else {
                        panic!("field published before refusal");
                    };
                    let poison = poison.clone();
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
                for name in ["third", "end", "Item"] {
                    assert!(matches!(
                        resolver
                            .declarations
                            .state(&CanonicalTarget::Source(defs[name])),
                        FactState::Unperformed
                    ));
                }
            },
        );
    }
}
