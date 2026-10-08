use super::*;

fn with_nodes(inspect: impl FnOnce(&TraitEvidence, &[EvidenceProofId])) {
    with_evidence(
        &chain_source(8),
        "direct",
        |declarations, signatures, environment, owner| {
            warm_chain(declarations, signatures, environment, owner, 8);
        },
        |evidence, declarations| {
            let mut ids = Vec::new();
            let mut next = Some(root_proof(evidence, declarations, "Hop7"));
            while let Some(id) = next {
                ids.push(id);
                let EvidenceSource::Implementation {
                    implementation_bounds,
                    ..
                } = evidence.proof(id).unwrap()
                else {
                    panic!("a completed chain contains implementation nodes")
                };
                next = implementation_bounds.first().copied();
            }
            ids.reverse();
            assert_eq!(ids.len(), 8);
            inspect(evidence, &ids);
        },
    );
}

fn node(
    evidence: &TraitEvidence,
    template: EvidenceProofId,
    implementation: &[EvidenceProofId],
    traits: &[EvidenceProofId],
) -> EvidenceSource {
    let mut node = evidence.proof(template).unwrap().clone();
    let EvidenceSource::Implementation {
        implementation_bounds,
        trait_bounds,
        ..
    } = &mut node
    else {
        panic!("a node template is an implementation")
    };
    *implementation_bounds = implementation.into();
    *trait_bounds = traits.into();
    node
}

#[test]
fn ordered_diamond_preserves_postorder_ids_families_and_repeated_edges() {
    with_nodes(|evidence, ids| {
        let [leaf, left, right, root, ..] = *ids else {
            unreachable!()
        };
        let nodes = BTreeMap::from([
            (leaf, node(evidence, leaf, &[], &[])),
            (left, node(evidence, left, &[leaf, leaf], &[])),
            (right, node(evidence, right, &[leaf], &[left, leaf])),
            (
                root,
                node(
                    evidence,
                    root,
                    &[right, left, leaf, right],
                    &[left, right, leaf, left],
                ),
            ),
        ]);
        let mut store = WitnessStore::default();
        let mut cache = BTreeMap::new();
        let mut active = BTreeSet::new();
        assert_eq!(
            compare(
                &mut store,
                &|id| nodes.get(&id),
                root,
                &Witnesses::new(),
                &mut cache,
                &mut active
            ),
            Some(WitnessId(3))
        );
        assert_eq!(
            cache,
            BTreeMap::from([
                (leaf, WitnessId(0)),
                (left, WitnessId(1)),
                (right, WitnessId(2)),
                (root, WitnessId(3))
            ])
        );
        let value = store.value(WitnessId(3)).unwrap();
        assert_eq!(
            &*value.implementation_bounds,
            [WitnessId(2), WitnessId(1), WitnessId(0), WitnessId(2)]
        );
        assert_eq!(
            &*value.trait_bounds,
            [WitnessId(1), WitnessId(2), WitnessId(0), WitnessId(1)]
        );
        assert_eq!(
            &*store.value(WitnessId(1)).unwrap().implementation_bounds,
            [WitnessId(0), WitnessId(0)]
        );
        assert!(active.is_empty());
    });
}

#[test]
fn distinct_proof_ids_can_intern_the_same_witness_value() {
    with_nodes(|evidence, ids| {
        let [first, second, root, ..] = *ids else {
            unreachable!()
        };
        let leaf = node(evidence, first, &[], &[]);
        let nodes = BTreeMap::from([
            (first, leaf.clone()),
            (second, leaf),
            (root, node(evidence, root, &[second, first, second], &[])),
        ]);
        let mut store = WitnessStore::default();
        let mut cache = BTreeMap::new();
        assert_eq!(
            compare(
                &mut store,
                &|id| nodes.get(&id),
                root,
                &Witnesses::new(),
                &mut cache,
                &mut BTreeSet::new()
            ),
            Some(WitnessId(1))
        );
        assert_eq!(store.values.len(), 2);
        assert_eq!(cache.len(), 3);
        assert_eq!(cache[&first], cache[&second]);
        assert_eq!(
            &*store.value(WitnessId(1)).unwrap().implementation_bounds,
            [WitnessId(0); 3]
        );
    });
}

#[test]
fn interning_distinguishes_target_substitution_family_order_and_multiplicity() {
    with_nodes(|evidence, ids| {
        let mut store = WitnessStore::default();
        for &proof in &ids[..2] {
            let value = node(evidence, proof, &[], &[]);
            assert!(
                compare(
                    &mut store,
                    &|_| Some(&value),
                    proof,
                    &Witnesses::new(),
                    &mut BTreeMap::new(),
                    &mut BTreeSet::new()
                )
                .is_some()
            );
        }
        let base = store.value(WitnessId(1)).unwrap().clone();
        let other = store.value(WitnessId(0)).unwrap().clone();
        assert_ne!(base.implementation, other.implementation);
        assert_ne!(base.substitution, other.substitution);

        let mut variants = vec![base.clone()];
        variants.push(WitnessValue {
            implementation: other.implementation,
            ..base.clone()
        });
        variants.push(WitnessValue {
            substitution: other.substitution,
            ..base.clone()
        });
        variants.push(WitnessValue {
            implementation_bounds: Box::new([WitnessId(0), WitnessId(1)]),
            ..base.clone()
        });
        variants.push(WitnessValue {
            implementation_bounds: Box::new([WitnessId(1), WitnessId(0)]),
            ..base.clone()
        });
        variants.push(WitnessValue {
            trait_bounds: Box::new([WitnessId(0), WitnessId(1)]),
            ..base.clone()
        });
        variants.push(WitnessValue {
            implementation_bounds: Box::new([WitnessId(0), WitnessId(1), WitnessId(0)]),
            ..base
        });
        let result = variants
            .iter()
            .map(|value| store.intern(value.clone()))
            .collect::<Vec<_>>();
        assert_eq!(
            result.iter().copied().collect::<BTreeSet<_>>().len(),
            variants.len()
        );
        assert_eq!(
            variants
                .into_iter()
                .map(|value| store.intern(value))
                .collect::<Vec<_>>(),
            result
        );
    });
}

#[test]
fn missing_nodes_and_cycles_preserve_completed_prefix_and_active_ancestors() {
    with_nodes(|evidence, ids| {
        let [leaf, bad, branch, untouched, root, ..] = *ids else {
            unreachable!()
        };
        for trait_family in [false, true] {
            for cyclic in [false, true] {
                let (implementation, traits) = if trait_family {
                    (vec![leaf], vec![branch, untouched])
                } else {
                    (vec![leaf, branch, untouched], vec![])
                };
                let mut nodes = BTreeMap::from([
                    (leaf, node(evidence, leaf, &[], &[])),
                    (branch, node(evidence, branch, &[bad], &[])),
                    (untouched, node(evidence, untouched, &[], &[])),
                    (root, node(evidence, root, &implementation, &traits)),
                ]);
                if cyclic {
                    nodes.insert(bad, node(evidence, bad, &[root], &[]));
                }
                let mut store = WitnessStore::default();
                let mut cache = BTreeMap::new();
                let mut active = BTreeSet::new();
                assert_eq!(
                    compare(
                        &mut store,
                        &|id| nodes.get(&id),
                        root,
                        &Witnesses::new(),
                        &mut cache,
                        &mut active
                    ),
                    None
                );
                assert_eq!(cache, BTreeMap::from([(leaf, WitnessId(0))]));
                assert_eq!(store.values.len(), 1);
                assert_eq!(active, BTreeSet::from([root, branch, bad]));
                assert!(!active.contains(&untouched));
            }
        }
    });
}

#[test]
fn cached_proof_precedes_active_state_and_input_lookup() {
    with_nodes(|_, ids| {
        let root = ids[0];
        let cached = WitnessId(99);
        let mut store = WitnessStore::default();
        let mut cache = BTreeMap::from([(root, cached)]);
        let mut active = BTreeSet::from([root]);
        assert_eq!(
            compare(
                &mut store,
                &|_| panic!("cached evidence is not read"),
                root,
                &Witnesses::new(),
                &mut cache,
                &mut active
            ),
            Some(cached)
        );
        assert_eq!(active, BTreeSet::from([root]));
        assert!(store.values.is_empty());
    });
}

#[test]
fn active_uncached_proof_is_rejected_without_reading_input() {
    with_nodes(|_, ids| {
        let root = ids[0];
        let mut store = WitnessStore::default();
        let mut cache = BTreeMap::new();
        let mut active = BTreeSet::from([root]);
        assert_eq!(
            compare(
                &mut store,
                &|_| panic!("active evidence is not read"),
                root,
                &Witnesses::new(),
                &mut cache,
                &mut active
            ),
            None
        );
        assert_eq!(active, BTreeSet::from([root]));
        assert!(cache.is_empty());
        assert!(store.values.is_empty());
    });
}

#[test]
fn empty_families_and_preexisting_interned_values_reuse_exact_ids() {
    with_nodes(|evidence, ids| {
        let mut store = WitnessStore::default();
        for _ in 0..2 {
            for (index, &proof) in ids.iter().enumerate() {
                let value = node(evidence, proof, &[], &[]);
                let mut cache = BTreeMap::new();
                assert_eq!(
                    compare(
                        &mut store,
                        &|_| Some(&value),
                        proof,
                        &Witnesses::new(),
                        &mut cache,
                        &mut BTreeSet::new()
                    ),
                    Some(WitnessId(index))
                );
                assert_eq!(cache, BTreeMap::from([(proof, WitnessId(index))]));
            }
        }
        assert_eq!(store.values.len(), ids.len());
    });
}

#[test]
fn wide_repeated_families_resolve_once_without_deduplicating_edges() {
    with_nodes(|evidence, ids| {
        let root = ids[7];
        let implementation = (0..2048).map(|index| ids[index % 4]).collect::<Vec<_>>();
        let traits = implementation.iter().rev().copied().collect::<Vec<_>>();
        let mut nodes = ids[..4]
            .iter()
            .map(|&id| (id, node(evidence, id, &[], &[])))
            .collect::<BTreeMap<_, _>>();
        nodes.insert(root, node(evidence, root, &implementation, &traits));
        let mut store = WitnessStore::default();
        let mut cache = BTreeMap::new();
        assert_eq!(
            compare(
                &mut store,
                &|id| nodes.get(&id),
                root,
                &Witnesses::new(),
                &mut cache,
                &mut BTreeSet::new()
            ),
            Some(WitnessId(4))
        );
        let value = store.value(WitnessId(4)).unwrap();
        assert_eq!(
            &*value.implementation_bounds,
            implementation
                .iter()
                .map(|id| cache[id])
                .collect::<Vec<_>>()
        );
        assert_eq!(
            &*value.trait_bounds,
            traits.iter().map(|id| cache[id]).collect::<Vec<_>>()
        );
        assert_eq!(cache.len(), 5);
    });
}

#[test]
fn bounded_dag_matrix_matches_recursive_values_ids_reads_and_failure_prefixes() {
    with_nodes(|evidence, ids| {
        for seed in 0..128_usize {
            let nodes = ids
                .iter()
                .enumerate()
                .map(|(index, &id)| {
                    let mut implementation = Vec::new();
                    let mut traits = Vec::new();
                    for (child_index, &child) in ids[..index].iter().enumerate() {
                        if (seed >> ((index + child_index) % 7)) & 1 != 0 {
                            implementation.push(child);
                            if seed & 2 != 0 {
                                implementation.push(child);
                            }
                        }
                        if (seed >> ((index * 3 + child_index) % 7)) & 1 != 0 {
                            traits.push(child);
                        }
                    }
                    traits.reverse();
                    (id, node(evidence, id, &implementation, &traits))
                })
                .collect::<BTreeMap<_, _>>();

            for missing in std::iter::once(None).chain(ids.iter().copied().map(Some)) {
                let mut store = WitnessStore::default();
                let lookup = |id| {
                    if Some(id) == missing {
                        None
                    } else {
                        nodes.get(&id)
                    }
                };
                compare(
                    &mut store,
                    &lookup,
                    ids[7],
                    &Witnesses::new(),
                    &mut BTreeMap::new(),
                    &mut BTreeSet::new(),
                );
            }
        }
    });
}

#[test]
fn deep_trait_bound_chain_reconstructs_without_native_recursion() {
    let depth = 4096;
    with_evidence(
        &chain_source(depth),
        "direct",
        |declarations, signatures, environment, owner| {
            warm_chain(declarations, signatures, environment, owner, depth)
        },
        |evidence, declarations| {
            let root = root_proof(evidence, declarations, &format!("Hop{}", depth - 1));
            let mut next = Some(root);
            let mut nodes = BTreeMap::new();
            while let Some(id) = next {
                let mut node = evidence.proof(id).unwrap().clone();
                let EvidenceSource::Implementation {
                    implementation_bounds,
                    trait_bounds,
                    ..
                } = &mut node
                else {
                    unreachable!()
                };
                std::mem::swap(implementation_bounds, trait_bounds);
                next = trait_bounds.first().copied();
                nodes.insert(id, node);
            }
            let mut store = WitnessStore::default();
            let mut cache = BTreeMap::new();
            let mut active = BTreeSet::new();
            assert_eq!(
                store.resolve_inner(
                    &|id| nodes.get(&id),
                    root,
                    &Witnesses::new(),
                    &mut cache,
                    &mut active
                ),
                Some(WitnessId(depth - 1))
            );
            assert_eq!(store.values.len(), depth);
            assert_eq!(cache.len(), depth);
            assert!(active.is_empty());
            assert_eq!(
                &*store.value(WitnessId(depth - 1)).unwrap().trait_bounds,
                [WitnessId(depth - 2)]
            );
        },
    );
}
