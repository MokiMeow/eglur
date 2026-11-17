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

fn publish_enum(resolver: &mut SignatureResolver<'_, '_>, owner: DefId, members: &[DefId]) {
    resolver
        .declarations
        .insert(
            CanonicalTarget::Source(owner),
            SemanticFact::Known(DeclarationSignature::Enum {
                generic_parameters: Box::new([]),
                variants: members
                    .iter()
                    .copied()
                    .map(CanonicalTarget::Source)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            }),
        )
        .unwrap();
}

fn publish_variant(resolver: &mut SignatureResolver<'_, '_>, owner: DefId, id: DefId, name: &str) {
    resolver
        .declarations
        .insert(
            CanonicalTarget::Source(id),
            SemanticFact::Known(DeclarationSignature::EnumVariant {
                owner: CanonicalTarget::Source(owner),
                name: NameKey::new(name, false).unwrap(),
                payload: EnumVariantPayloadSignature::Absent,
            }),
        )
        .unwrap();
}

#[test]
fn enum_name_fold_completes_once_and_preserves_partial_members() {
    for count in [0, 8, 32, 128] {
        for mode in [
            "known",
            "distinct",
            "repeat",
            "overlap",
            "partial",
            "notapplicable",
            "partial-first-poison",
            "partial-last-poison",
        ] {
            if count == 0 && mode != "known" {
                continue;
            }
            let variants = (0..count)
                .map(|index| format!("V{index}\n"))
                .collect::<String>();
            let text = format!("enum E {{ {variants}Last }}\nstruct After {{}}\n");
            with_resolver(&[&text], |resolver, _| {
                let defs = resolver
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
                let owner = defs["E"];
                let members = (0..count)
                    .map(|index| defs[&format!("V{index}")])
                    .chain([defs["Last"]])
                    .collect::<Vec<_>>();
                publish_enum(resolver, owner, &members);
                publish_variant(resolver, owner, defs["Last"], "Last");
                resolver
                    .declarations
                    .insert(
                        CanonicalTarget::Source(defs["After"]),
                        SemanticFact::Known(DeclarationSignature::Nominal {
                            generic_parameters: Box::new([]),
                        }),
                    )
                    .unwrap();
                let inputs = roots(resolver.diagnostics, count);
                let all = inputs
                    .iter()
                    .cloned()
                    .reduce(|left, right| left.merge(&right).unwrap());
                let mut included = 0;
                for index in 0..count {
                    let target = CanonicalTarget::Source(members[index]);
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
                    if let Some(poison) = poison {
                        included += 1;
                        resolver
                            .declarations
                            .insert(target, SemanticFact::Poisoned(poison))
                            .unwrap();
                    } else if mode == "known" {
                        publish_variant(resolver, owner, members[index], &format!("V{index}"));
                    } else if mode == "notapplicable" {
                        resolver
                            .declarations
                            .insert(target, SemanticFact::NotApplicable)
                            .unwrap();
                    }
                }
                WORK.set(Work::default());
                resolver.index_enum_variant_names().unwrap();
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: included,
                        completed: usize::from(included != 0)
                    }
                );
                assert!(matches!(
                    resolver
                        .enum_variant_names
                        .get(&CanonicalTarget::Source(defs["After"])),
                    Some(SemanticFact::NotApplicable)
                ));
                let fact = resolver
                    .enum_variant_names
                    .get(&CanonicalTarget::Source(owner));
                if mode == "known" {
                    let Some(SemanticFact::Known(names)) = fact else {
                        panic!("known variant names");
                    };
                    assert_eq!(names.len(), count + 1);
                    for index in 0..count {
                        assert!(names.contains(&NameKey::new(format!("V{index}"), false).unwrap()));
                    }
                    assert!(names.contains(&NameKey::new("Last", false).unwrap()));
                } else if included == 0 {
                    assert!(fact.is_none());
                } else {
                    let Some(SemanticFact::Poisoned(poison)) = fact else {
                        panic!("poison dominates partial members");
                    };
                    let poison = poison.clone();
                    let mut store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
                    store
                        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
                        .unwrap();
                    store
                        .select_preceding_roots(&poison, all.as_ref().unwrap())
                        .unwrap();
                    let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(None) else {
                        panic!("retained member roots");
                    };
                    let actual = snapshot
                        .diagnostics()
                        .iter()
                        .map(|diagnostic| diagnostic.code().as_str().to_owned())
                        .collect::<BTreeSet<_>>();
                    let expected = if mode == "distinct" || mode == "overlap" {
                        (0..count).map(|index| format!("ROOT_{index:03}")).collect()
                    } else {
                        BTreeSet::from(["ROOT_000".to_owned()])
                    };
                    assert_eq!(actual, expected);
                }
            });
        }
    }
}

#[test]
fn foreign_enum_name_error_preserves_prior_index_and_refuses_later_entries() {
    for foreign_first in [false, true] {
        with_resolver(
            &["enum Before { Ready }\nenum E { First\nSecond\nThird }\nstruct After {}\n"],
            |resolver, _| {
                let defs = resolver
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
                publish_enum(resolver, defs["Before"], &[defs["Ready"]]);
                publish_variant(resolver, defs["Before"], defs["Ready"], "Ready");
                publish_enum(
                    resolver,
                    defs["E"],
                    &[defs["First"], defs["Second"], defs["Third"]],
                );
                resolver
                    .declarations
                    .insert(
                        CanonicalTarget::Source(defs["After"]),
                        SemanticFact::Known(DeclarationSignature::Nominal {
                            generic_parameters: Box::new([]),
                        }),
                    )
                    .unwrap();
                let own = roots(resolver.diagnostics, 1);
                let mut foreign_store = DiagnosticStore::new();
                let foreign = roots(&mut foreign_store, 1);
                for (name, is_foreign) in [("First", foreign_first), ("Second", !foreign_first)] {
                    resolver
                        .declarations
                        .insert(
                            CanonicalTarget::Source(defs[name]),
                            SemanticFact::Poisoned(if is_foreign {
                                foreign[0].clone()
                            } else {
                                own[0].clone()
                            }),
                        )
                        .unwrap();
                }
                WORK.set(Work::default());
                assert_eq!(
                    resolver.index_enum_variant_names().unwrap_err(),
                    SignatureResolutionError::DiagnosticStore(DiagnosticStoreError::ForeignPoison)
                );
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: 2,
                        completed: 0
                    }
                );
                let Some(SemanticFact::Known(names)) = resolver
                    .enum_variant_names
                    .get(&CanonicalTarget::Source(defs["Before"]))
                else {
                    panic!("earlier index remains published");
                };
                assert_eq!(
                    names,
                    &HashSet::from([NameKey::new("Ready", false).unwrap()])
                );
                assert!(matches!(
                    resolver
                        .enum_variant_names
                        .get(&CanonicalTarget::Source(defs["Ready"])),
                    Some(SemanticFact::NotApplicable)
                ));
                for name in ["E", "First", "Second", "Third", "After"] {
                    assert!(
                        !resolver
                            .enum_variant_names
                            .contains_key(&CanonicalTarget::Source(defs[name]))
                    );
                }
            },
        );
    }
}

#[test]
fn poisoned_enum_fast_path_retains_roots_without_member_fold() {
    with_resolver(&["enum E { Ready }\n"], |resolver, _| {
        let owner = resolver
            .collection
            .modules()
            .next()
            .unwrap()
            .declarations()
            .find(|declaration| declaration.kind() == DeclarationKind::Enum)
            .unwrap()
            .id();
        let poison = roots(resolver.diagnostics, 1).pop().unwrap();
        resolver
            .declarations
            .insert(
                CanonicalTarget::Source(owner),
                SemanticFact::Poisoned(poison),
            )
            .unwrap();
        WORK.set(Work::default());
        resolver.index_enum_variant_names().unwrap();
        let Some(SemanticFact::Poisoned(poison)) = resolver
            .enum_variant_names
            .get(&CanonicalTarget::Source(owner))
        else {
            panic!("poisoned owner index");
        };
        resolver
            .diagnostics
            .record_failure(DiagnosticFailure::Derivative(poison.clone()))
            .unwrap();
        assert_eq!(WORK.get(), Work::default());
    });
}
