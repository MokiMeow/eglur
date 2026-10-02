use super::*;
use crate::{
    AssociatedProjection, ExternalDefId, GenericParameterId, IntrinsicNominal, NominalHead,
    PrimitiveType, RawPointerMutability, ReferenceMutability, TypeKind,
};

type CycleSets = (BTreeSet<CanonicalTarget>, BTreeSet<CanonicalTarget>);

#[derive(Debug, Default, Eq, PartialEq)]
struct Work {
    roots: usize,
    payloads: usize,
    payload_edges: usize,
    projections: usize,
    indexed_edges: usize,
}

impl Work {
    fn record(&mut self, step: DependencyStep) {
        match step {
            DependencyStep::RootValidated => self.roots += 1,
            DependencyStep::PayloadInspected => self.payloads += 1,
            DependencyStep::PayloadEdgeScheduled => self.payload_edges += 1,
            DependencyStep::ProjectionLinked => self.projections += 1,
            DependencyStep::EdgeIndexed => self.indexed_edges += 1,
        }
    }

    fn verify(&self, graph: &BindingDependencies) {
        assert_eq!(graph.edges.len(), graph.bindings.len() + self.payloads);
        assert_eq!(
            graph.edges.iter().map(Vec::len).sum::<usize>(),
            self.indexed_edges
        );
        assert_eq!(
            self.indexed_edges,
            self.roots + self.payload_edges + self.projections
        );
    }
}

fn external(name: &str) -> CanonicalTarget {
    CanonicalTarget::External(ExternalDefId::new(name).unwrap())
}

fn context(store: &mut CanonicalTypeStore) -> (TypeId, TraitRef) {
    let target = store.self_type(external("std::Node")).unwrap();
    let arguments = store.intern_substitution([]).unwrap();
    (target, TraitRef::new(external("std::Linked"), arguments))
}

fn terminal(
    store: &mut CanonicalTypeStore,
    target: TypeId,
    trait_ref: &TraitRef,
    associated: CanonicalTarget,
) -> TypeId {
    store
        .associated_projection(AssociatedProjection::new(
            target,
            trait_ref.clone(),
            associated,
        ))
        .unwrap()
}

fn binding(associated: CanonicalTarget, value: TypeId) -> AssociatedBindingSignature {
    AssociatedBindingSignature {
        declaration: associated.clone(),
        associated,
        value,
    }
}

fn sets(graph: &BindingDependencies) -> CycleSets {
    graph
        .cycle_sets()
        .map(|cycles| (cycles.cyclic, cycles.affected))
        .unwrap_or_default()
}

fn observed(
    store: &CanonicalTypeStore,
    trait_ref: &TraitRef,
    target: TypeId,
    bindings: &[AssociatedBindingSignature],
) -> (BindingDependencies, Work) {
    let mut work = Work::default();
    let graph = BindingDependencies::new_observed(store, trait_ref, target, bindings, |step| {
        work.record(step)
    })
    .unwrap();
    work.verify(&graph);
    assert_eq!(
        sets(&graph),
        sets(&BindingDependencies::new(store, trait_ref, target, bindings).unwrap())
    );
    (graph, work)
}

fn positive_paths(targets: &[CanonicalTarget], edges: &[Vec<usize>]) -> CycleSets {
    let mut reachable = vec![vec![false; targets.len()]; targets.len()];
    for (node, row) in edges.iter().enumerate() {
        for &next in row {
            reachable[node][next] = true;
        }
    }
    for via in 0..targets.len() {
        for from in 0..targets.len() {
            for to in 0..targets.len() {
                reachable[from][to] |= reachable[from][via] && reachable[via][to];
            }
        }
    }
    let cyclic = (0..targets.len())
        .filter(|&node| reachable[node][node])
        .collect::<BTreeSet<_>>();
    let affected =
        (0..targets.len()).filter(|&node| cyclic.iter().any(|&cycle| reachable[node][cycle]));
    (
        cyclic.iter().map(|&node| targets[node].clone()).collect(),
        affected.map(|node| targets[node].clone()).collect(),
    )
}

// The pre-sharing relation is a bounded differential oracle, not a production interpreter.
fn materialized_sets(
    store: &CanonicalTypeStore,
    trait_ref: &TraitRef,
    target: TypeId,
    bindings: &[AssociatedBindingSignature],
) -> Result<(CycleSets, usize), TypeStoreError> {
    let targets = bindings
        .iter()
        .map(|binding| binding.associated.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let indices = targets
        .iter()
        .enumerate()
        .map(|(index, node)| (node, index))
        .collect::<BTreeMap<_, _>>();
    let mut edges = vec![BTreeSet::new(); targets.len()];
    let mut reads = 0;
    for binding in bindings {
        store.kind(binding.value)?;
        let mut pending = vec![binding.value];
        let mut seen = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(current) {
                continue;
            }
            reads += 1;
            match store.kind(current)? {
                TypeKind::Never
                | TypeKind::Primitive(_)
                | TypeKind::GenericParameter(_)
                | TypeKind::SelfType(_) => {}
                TypeKind::Nominal { substitution, .. } => {
                    pending.extend(store.substitution(*substitution)?.iter().copied())
                }
                TypeKind::Reference { referent, .. } | TypeKind::RawPointer { referent, .. } => {
                    pending.push(*referent)
                }
                TypeKind::Tuple(members) | TypeKind::ErrorUnion(members) => {
                    pending.extend(members.iter().copied())
                }
                TypeKind::Callable { parameters, result } => {
                    pending.extend(parameters.iter().copied());
                    pending.push(*result);
                }
                TypeKind::AssociatedProjection(projection) => {
                    if projection.base() == target
                        && projection.trait_ref() == trait_ref
                        && let Some(&next) = indices.get(projection.associated())
                    {
                        edges[indices[&binding.associated]].insert(next);
                    }
                }
            }
        }
    }
    let edges = edges
        .into_iter()
        .map(|row| row.into_iter().collect())
        .collect::<Vec<_>>();
    Ok((positive_paths(&targets, &edges), reads))
}

#[test]
fn all_binding_graphs_through_four_nodes_preserve_positive_paths() {
    let mut checked = 0;
    for size in 0..=4 {
        for mask in 0..(1_usize << (size * size)) {
            let mut store = CanonicalTypeStore::new().unwrap();
            let (target, trait_ref) = context(&mut store);
            let targets = (0..size)
                .map(|node| external(&format!("std::Linked::A{node}")))
                .collect::<Vec<_>>();
            let projections = targets
                .iter()
                .map(|node| terminal(&mut store, target, &trait_ref, node.clone()))
                .collect::<Vec<_>>();
            let edges = (0..size)
                .map(|node| {
                    (0..size)
                        .filter(|&next| mask & (1 << (node * size + next)) != 0)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let bindings = targets
                .iter()
                .zip(&edges)
                .map(|(node, row)| {
                    binding(
                        node.clone(),
                        store
                            .tuple(row.iter().map(|&next| projections[next]))
                            .unwrap(),
                    )
                })
                .collect::<Vec<_>>();
            let (graph, _) = observed(&store, &trait_ref, target, &bindings);
            assert_eq!(sets(&graph), positive_paths(&targets, &edges));
            assert_eq!(
                sets(&graph),
                materialized_sets(&store, &trait_ref, target, &bindings)
                    .unwrap()
                    .0
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 66_067);
}

#[test]
fn shared_growing_payloads_do_not_materialize_dense_binding_inventories() {
    for size in [32, 128, 512] {
        let mut store = CanonicalTypeStore::new().unwrap();
        let (target, trait_ref) = context(&mut store);
        let targets = (0..size)
            .map(|node| external(&format!("std::Linked::A{node}")))
            .collect::<Vec<_>>();
        let projections = targets
            .iter()
            .map(|node| terminal(&mut store, target, &trait_ref, node.clone()))
            .collect::<Vec<_>>();
        let mut root = projections[0];
        for &projection in &projections[1..] {
            root = store.tuple([root, projection]).unwrap();
        }
        let mut bindings = targets
            .iter()
            .map(|node| binding(node.clone(), root))
            .collect::<Vec<_>>();
        let (graph, work) = observed(&store, &trait_ref, target, &bindings);
        let (expected, old_reads) =
            materialized_sets(&store, &trait_ref, target, &bindings).unwrap();
        assert_eq!(sets(&graph), expected);
        assert_eq!(old_reads, size * (2 * size - 1));
        assert_eq!(work.roots, size);
        assert_eq!(work.payloads, 2 * size - 1);
        assert_eq!(work.payload_edges, 2 * (size - 1));
        assert_eq!(work.projections, size);
        assert_eq!(graph.edges.len(), 3 * size - 1);
        assert_eq!(work.indexed_edges, 4 * size - 2);

        bindings.reverse();
        let (reversed, reversed_work) = observed(&store, &trait_ref, target, &bindings);
        assert_eq!(sets(&reversed), expected);
        assert_eq!(reversed_work, work);
    }
}

#[test]
fn distinct_suffix_roots_share_payload_vertices_with_zero_or_one_projection() {
    for size in [32, 128, 512] {
        for with_projection in [false, true] {
            let mut store = CanonicalTypeStore::new().unwrap();
            let (target, trait_ref) = context(&mut store);
            let first = external("std::Linked::A0");
            let scalar = store.primitive(PrimitiveType::I64).unwrap();
            let mut root = if with_projection {
                terminal(&mut store, target, &trait_ref, first)
            } else {
                scalar
            };
            let mut bindings = Vec::new();
            for node in 0..size {
                root = store.tuple([root, scalar]).unwrap();
                bindings.push(binding(external(&format!("std::Linked::A{node}")), root));
            }
            let (graph, work) = observed(&store, &trait_ref, target, &bindings);
            let (expected, old_reads) =
                materialized_sets(&store, &trait_ref, target, &bindings).unwrap();
            assert_eq!(sets(&graph), expected);
            assert_eq!(
                old_reads,
                size * (size + 1) / 2 + size * (1 + usize::from(with_projection))
            );
            assert_eq!(work.roots, size);
            assert_eq!(work.payloads, size + 1 + usize::from(with_projection));
            assert_eq!(work.payload_edges, 2 * size);
            assert_eq!(work.projections, usize::from(with_projection));
            assert_eq!(work.indexed_edges, 3 * size + usize::from(with_projection));
        }
    }
}

#[test]
fn heterogeneous_payloads_duplicates_and_complete_projection_keys_stay_exact() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let (target, trait_ref) = context(&mut store);
    let first = external("std::Linked::A");
    let second = external("std::Linked::B");
    let projection = terminal(&mut store, target, &trait_ref, first.clone());
    let scalar = store.primitive(PrimitiveType::Str).unwrap();
    let parameter = store
        .generic_parameter(GenericParameterId::Trusted {
            owner: ExternalDefId::new("std::generic").unwrap(),
            index: 0,
        })
        .unwrap();
    let reference = store
        .reference(ReferenceMutability::Shared, projection)
        .unwrap();
    let pointer = store
        .raw_pointer(RawPointerMutability::Mutable, projection)
        .unwrap();
    let nominal = store
        .nominal(
            NominalHead::Intrinsic(IntrinsicNominal::Res),
            [reference, pointer],
        )
        .unwrap();
    let callable = store.callable([parameter, nominal], projection).unwrap();
    let union = store.error_union([callable, projection, scalar]).unwrap();
    let root = store
        .tuple([union, store.never(), target, projection, projection])
        .unwrap();
    let bindings = [
        binding(first.clone(), root),
        binding(second.clone(), root),
        binding(second.clone(), projection),
    ];
    let (graph, work) = observed(&store, &trait_ref, target, &bindings);
    assert_eq!(
        sets(&graph),
        (
            BTreeSet::from([first.clone()]),
            BTreeSet::from([first.clone(), second])
        )
    );
    assert_eq!(
        sets(&graph),
        materialized_sets(&store, &trait_ref, target, &bindings)
            .unwrap()
            .0
    );
    assert_eq!(work.roots, 3);
    assert_eq!(work.projections, 1);

    let other_arguments = store.intern_substitution([root]).unwrap();
    let different_arguments = TraitRef::new(trait_ref.declaration().clone(), other_arguments);
    let different_trait = TraitRef::new(external("std::Other"), trait_ref.substitution());
    for (base, declaring_trait, associated) in [
        (root, trait_ref.clone(), first.clone()),
        (target, different_arguments, first.clone()),
        (target, different_trait, first.clone()),
        (target, trait_ref.clone(), external("std::Linked::Unknown")),
    ] {
        let unmatched = terminal(&mut store, base, &declaring_trait, associated);
        let bindings = [binding(first.clone(), unmatched)];
        let (graph, work) = observed(&store, &trait_ref, target, &bindings);
        assert_eq!(sets(&graph), CycleSets::default());
        assert_eq!(
            sets(&graph),
            materialized_sets(&store, &trait_ref, target, &bindings)
                .unwrap()
                .0
        );
        assert_eq!(work.payloads, 1);
        assert_eq!(work.payload_edges, 0);
        assert_eq!(work.projections, 0);
        assert_eq!(work.indexed_edges, 1);
    }
}

#[test]
fn unknown_roots_keep_written_order_before_any_sorted_graph_indexing() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let (target, trait_ref) = context(&mut store);
    let first = TypeId::from_index(store.type_count() + 10).unwrap();
    let second = TypeId::from_index(store.type_count() + 20).unwrap();
    for roots in [[first, second], [second, first]] {
        let bindings = [
            binding(external("std::Linked::Z"), target),
            binding(external("std::Linked::B"), roots[0]),
            binding(external("std::Linked::A"), roots[1]),
        ];
        assert_eq!(
            BindingDependencies::new(&store, &trait_ref, target, &bindings).err(),
            Some(TypeStoreError::UnknownType(roots[0]))
        );
        assert_eq!(
            materialized_sets(&store, &trait_ref, target, &bindings),
            Err(TypeStoreError::UnknownType(roots[0]))
        );
    }
}

#[test]
fn deep_payloads_do_not_visit_or_allocate_indices_for_unused_store_nodes() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let (target, trait_ref) = context(&mut store);
    let associated = external("std::Linked::A");
    let mut root = terminal(&mut store, target, &trait_ref, associated.clone());
    for _ in 0..20_000 {
        root = store.tuple([root]).unwrap();
    }
    let bindings = [binding(associated.clone(), root)];
    let (before, work) = observed(&store, &trait_ref, target, &bindings);
    let owner = ExternalDefId::new("std::unused").unwrap();
    for index in 0..20_000 {
        store
            .generic_parameter(GenericParameterId::Trusted {
                owner: owner.clone(),
                index,
            })
            .unwrap();
    }
    let (after, later_work) = observed(&store, &trait_ref, target, &bindings);
    assert_eq!(
        sets(&before),
        (
            BTreeSet::from([associated.clone()]),
            BTreeSet::from([associated])
        )
    );
    assert_eq!(after.edges, before.edges);
    assert_eq!(sets(&after), sets(&before));
    assert_eq!(later_work, work);
    assert_eq!(work.roots, 1);
    assert_eq!(work.payloads, 20_001);
    assert_eq!(work.payload_edges, 20_000);
    assert_eq!(work.projections, 1);
    assert_eq!(work.indexed_edges, 20_002);
}
