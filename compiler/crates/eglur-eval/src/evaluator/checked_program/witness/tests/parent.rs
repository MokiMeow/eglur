use super::*;
use eglur_sema::WitnessSlotId;

fn with_bindings(inspect: impl FnOnce(&TraitEvidence)) {
    with_evidence(
        concat!(
            "trait Leaf {}\nimpl Leaf for i64 {}\nimpl Leaf for Bool {}\n",
            "fn accept[T: Leaf](value: T) -> Unit = ()\n",
            "fn forward[T: Leaf](value: T) -> Unit = accept(value)\n",
            "fn other[T: Leaf](value: T) -> Unit = accept(value)\n",
            "fn direct() -> Unit { forward(0)\nforward(true)\nother(0)\nyield () }\n",
        ),
        "direct",
        |_, _, _, _| {},
        |evidence, _| inspect(evidence),
    );
}

fn bound_proofs(evidence: &TraitEvidence) -> Vec<(EvidenceProofId, WitnessSlotId)> {
    evidence
        .environments()
        .flat_map(|environment| environment.entries())
        .filter_map(|entry| match evidence.proof(entry.proof()).unwrap() {
            EvidenceSource::Bound(slot) => Some((entry.proof(), *slot)),
            EvidenceSource::Implementation { .. } => None,
        })
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .collect()
}

#[test]
fn bound_witnesses_are_opaque_and_each_resolve_uses_current_parent() {
    with_bindings(|evidence| {
        let (proof, slot) = bound_proofs(evidence)[0];
        let mut store = WitnessStore::default();
        for id in [WitnessId(41), WitnessId(73), WitnessId(41)] {
            let parent = Witnesses::from([(slot, id)]);
            assert_eq!(store.resolve_proof(evidence, proof, &parent), Some(id));
            assert_eq!(
                compare(
                    &mut store,
                    &|id| evidence.proof(id),
                    proof,
                    &parent,
                    &mut BTreeMap::new(),
                    &mut BTreeSet::new()
                ),
                Some(id)
            );
            assert!(store.values.is_empty());
            assert!(store.ids.is_empty());
        }
    });
}

#[test]
fn bound_slot_identity_includes_owner_not_just_local_index() {
    with_bindings(|evidence| {
        let proofs = bound_proofs(evidence);
        assert_eq!(proofs.len(), 2);
        let [(first, first_slot), (second, second_slot)] = proofs.as_slice() else {
            unreachable!()
        };
        assert_eq!(first_slot.local(), second_slot.local());
        assert_ne!(first_slot.owner(), second_slot.owner());
        let parent = Witnesses::from([(*first_slot, WitnessId(51))]);
        let mut store = WitnessStore::default();
        assert_eq!(
            store.resolve_proof(evidence, *first, &parent),
            Some(WitnessId(51))
        );
        let mut active = BTreeSet::new();
        let mut cache = BTreeMap::new();
        assert_eq!(
            compare(
                &mut store,
                &|id| evidence.proof(id),
                *second,
                &parent,
                &mut cache,
                &mut active
            ),
            None
        );
        assert_eq!(active, BTreeSet::from([*second]));
        assert!(cache.is_empty());
        assert!(store.values.is_empty());
    });
}

#[test]
fn missing_parent_binding_keeps_completed_prefix_in_either_family() {
    with_bindings(|evidence| {
        let bounds = bound_proofs(evidence);
        let (bound, slot) = bounds[0];
        let untouched = bounds[1].0;
        let implementations = evidence
            .environments()
            .flat_map(|environment| environment.entries())
            .filter(|entry| {
                matches!(
                    evidence.proof(entry.proof()),
                    Some(EvidenceSource::Implementation { .. })
                )
            })
            .map(|entry| entry.proof())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let [leaf, root] = implementations.as_slice() else {
            panic!("fixture has two concrete implementations")
        };

        for trait_family in [false, true] {
            let mut root_node = evidence.proof(*root).unwrap().clone();
            let EvidenceSource::Implementation {
                implementation_bounds,
                trait_bounds,
                ..
            } = &mut root_node
            else {
                unreachable!()
            };
            if trait_family {
                *implementation_bounds = Box::new([*leaf]);
                *trait_bounds = Box::new([bound, untouched]);
            } else {
                *implementation_bounds = Box::new([*leaf, bound, untouched]);
            }
            let lookup = |id| {
                if id == *root {
                    Some(&root_node)
                } else {
                    evidence.proof(id)
                }
            };
            let mut store = WitnessStore::default();
            let mut cache = BTreeMap::new();
            let mut active = BTreeSet::new();
            assert_eq!(
                compare(
                    &mut store,
                    &lookup,
                    *root,
                    &Witnesses::new(),
                    &mut cache,
                    &mut active
                ),
                None
            );
            assert_eq!(cache, BTreeMap::from([(*leaf, WitnessId(0))]));
            assert_eq!(active, BTreeSet::from([*root, bound]));
            assert_eq!(store.values.len(), 1);

            let parent = Witnesses::from([(slot, WitnessId(73)), (bounds[1].1, WitnessId(74))]);
            assert_eq!(
                compare(&mut store, &lookup, *root, &parent, &mut cache, &mut active),
                None
            );
            assert_eq!(
                compare(
                    &mut store,
                    &lookup,
                    *root,
                    &parent,
                    &mut cache,
                    &mut BTreeSet::new()
                ),
                Some(WitnessId(1))
            );
            assert_eq!(store.values.len(), 2);
            assert_eq!(cache.len(), 4);
        }
    });
}

#[test]
fn child_destinations_never_replace_original_parent_inputs() {
    with_evidence(
        concat!(
            "trait Leaf {}\nimpl Leaf for i64 {}\nimpl Leaf for Bool {}\n",
            "fn forward[T: Leaf, A: Leaf](x: T, y: A) -> Unit = forward(y, x)\n",
            "fn direct() -> Unit = forward(0, true)\n",
        ),
        "direct",
        |_, _, _, _| {},
        |evidence, _| {
            let environment = evidence
                .environments()
                .find(|environment| {
                    environment.entries().len() == 2
                        && environment.entries().iter().all(|entry| {
                            matches!(
                                evidence.proof(entry.proof()),
                                Some(EvidenceSource::Bound(_))
                            )
                        })
                })
                .unwrap();
            let slots = environment
                .entries()
                .iter()
                .map(|entry| {
                    let EvidenceSource::Bound(slot) = evidence.proof(entry.proof()).unwrap() else {
                        unreachable!()
                    };
                    *slot
                })
                .collect::<Vec<_>>();
            let parent = Witnesses::from([(slots[0], WitnessId(10)), (slots[1], WitnessId(20))]);
            let expected = environment
                .entries()
                .iter()
                .zip(&slots)
                .map(|(entry, slot)| (entry.destination().unwrap(), parent[slot]))
                .collect::<Witnesses>();
            assert_eq!(environment.entries()[0].destination(), Some(slots[1]));
            let mut store = WitnessStore::default();
            assert_eq!(
                store.child_witnesses(evidence, environment, &parent),
                Some(expected)
            );
            let missing_second = Witnesses::from([(slots[0], WitnessId(10))]);
            assert_eq!(
                store.child_witnesses(evidence, environment, &missing_second),
                None
            );
            assert_eq!(parent[&slots[0]], WitnessId(10));
            assert_eq!(parent[&slots[1]], WitnessId(20));
            assert!(store.values.is_empty());
        },
    );
}

#[test]
fn independent_child_environments_do_not_reuse_parent_bound_cache() {
    with_bindings(|evidence| {
        let (proof, slot) = bound_proofs(evidence)[0];
        let bound_environment = evidence
            .environments()
            .find(|environment| {
                environment
                    .entries()
                    .iter()
                    .any(|entry| entry.proof() == proof)
            })
            .unwrap();
        let destination = bound_environment.entries()[0].destination().unwrap();
        let concrete = evidence
            .environments()
            .filter(|environment| {
                environment.entries().len() == 1
                    && environment.entries()[0].destination() == Some(slot)
                    && matches!(
                        evidence.proof(environment.entries()[0].proof()),
                        Some(EvidenceSource::Implementation { .. })
                    )
            })
            .collect::<Vec<_>>();
        assert_eq!(concrete.len(), 2);
        let mut store = WitnessStore::default();
        let mut results = Vec::new();
        for environment in concrete {
            let parent = store
                .child_witnesses(evidence, environment, &Witnesses::new())
                .unwrap();
            let child = store
                .child_witnesses(evidence, bound_environment, &parent)
                .unwrap();
            assert_eq!(child[&destination], parent[&slot]);
            results.push(child[&destination]);
        }
        assert_ne!(results[0], results[1]);
        assert_eq!(store.values.len(), 2);
    });
}
