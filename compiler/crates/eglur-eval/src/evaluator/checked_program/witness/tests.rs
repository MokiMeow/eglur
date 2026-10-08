use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::super::{WitnessId, WitnessStore, WitnessValue, Witnesses};
use eglur_sema::{CanonicalTarget, EvidenceProofId, EvidenceSource, TraitEvidence};

mod fixture;
mod graph;
mod parent;
mod recursive;

use fixture::{root_proof, warm_chain, with_evidence};

fn chain_source(depth: usize) -> String {
    assert!(depth > 0);
    let mut source = String::from("trait Hop0 {}\nimpl Hop0 for i64 {}\n");
    for level in 1..depth {
        writeln!(
            source,
            "trait Hop{level} {{}}\nimpl[T: Hop{}] Hop{level} for T {{}}",
            level - 1
        )
        .unwrap();
    }
    writeln!(
        source,
        "fn consume[T: Hop{}](value: T) -> Unit = ()\nfn direct() -> Unit = consume(0)",
        depth - 1
    )
    .unwrap();
    source
}

fn compare<'evidence>(
    store: &mut WitnessStore,
    lookup: &impl Fn(EvidenceProofId) -> Option<&'evidence EvidenceSource>,
    root: EvidenceProofId,
    parent: &Witnesses,
    cache: &mut BTreeMap<EvidenceProofId, WitnessId>,
    active: &mut BTreeSet<EvidenceProofId>,
) -> Option<WitnessId> {
    let before_values = store.values.clone();
    let before_ids = store.ids.clone();
    let before_cache = cache.clone();
    let before_active = active.clone();
    let reads = RefCell::new(Vec::new());
    let traced_lookup = |id| {
        reads.borrow_mut().push(id);
        lookup(id)
    };
    let expected = store.recursive_resolve(&traced_lookup, root, parent, cache, active);
    let expected_reads = std::mem::take(&mut *reads.borrow_mut());
    let expected_values = std::mem::replace(&mut store.values, before_values);
    let expected_ids = std::mem::replace(&mut store.ids, before_ids);
    let expected_cache = std::mem::replace(cache, before_cache);
    let expected_active = std::mem::replace(active, before_active);
    let actual = store.resolve_inner(&traced_lookup, root, parent, cache, active);
    assert_eq!(actual, expected);
    assert_eq!(store.values, expected_values);
    assert_eq!(store.ids, expected_ids);
    assert_eq!(*cache, expected_cache);
    assert_eq!(*active, expected_active);
    assert_eq!(*reads.borrow(), expected_reads);
    actual
}

#[test]
fn source_completed_chains_match_recursive_interning_and_cache_order() {
    for depth in [1, 2, 8, 24] {
        with_evidence(
            &chain_source(depth),
            "direct",
            |declarations, signatures, environment, owner| {
                warm_chain(declarations, signatures, environment, owner, depth);
            },
            |evidence, declarations| {
                let root = root_proof(evidence, declarations, &format!("Hop{}", depth - 1));
                let mut store = WitnessStore::default();
                let mut cache = BTreeMap::new();
                let mut active = BTreeSet::new();
                let id = compare(
                    &mut store,
                    &|id| evidence.proof(id),
                    root,
                    &Witnesses::new(),
                    &mut cache,
                    &mut active,
                )
                .unwrap();
                assert_eq!(id, WitnessId(depth - 1));
                assert_eq!(store.values.len(), depth);
                assert_eq!(cache.len(), depth);
                assert!(active.is_empty());
            },
        );
    }
}

#[test]
fn deep_source_completed_proof_dag_reconstructs_without_native_recursion() {
    let depth = 4096;
    with_evidence(
        &chain_source(depth),
        "direct",
        |declarations, signatures, environment, owner| {
            warm_chain(declarations, signatures, environment, owner, depth);
        },
        |evidence, declarations| {
            let root = root_proof(evidence, declarations, &format!("Hop{}", depth - 1));
            let mut store = WitnessStore::default();
            assert_eq!(
                store.resolve_proof(evidence, root, &Witnesses::new()),
                Some(WitnessId(depth - 1))
            );
            assert_eq!(store.values.len(), depth);
        },
    );
}
