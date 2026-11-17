use crate::record_reservation::observation::{assert_append, assert_growth_envelope, observe};
use crate::{ModuleId, WitnessSlotId};

use super::*;

#[test]
fn proof_records_reuse_dense_ids_and_preserve_contents_through_freeze() {
    let owner = DefId::from_index(ModuleId::from_index(0).unwrap(), 0).unwrap();
    let mut arena = ProofArena::default();
    let mut reservations = Vec::new();
    for index in 0..1025 {
        let node = EvidenceSource::Bound(WitnessSlotId::from_index(owner, index).unwrap());
        let (id, observed) = observe(|| arena.intern(node.clone()));
        let id = id.unwrap();
        assert_eq!(id.index(), index);
        assert_eq!(observed.len(), 1);
        assert_append(&observed[0], arena.nodes.len(), arena.nodes.capacity());
        assert_eq!(arena.nodes[id.index()], node);
        assert_eq!(arena.ids.get(&node), Some(&id));
        reservations.extend(observed);
        assert_growth_envelope(
            &reservations,
            arena.nodes.len() * size_of::<EvidenceSource>(),
        );

        let (hit, observed) = observe(|| arena.intern(node));
        assert_eq!(hit.unwrap(), id);
        assert!(observed.is_empty());
        assert_eq!(arena.nodes.len(), index + 1);
    }
    let expected = arena.nodes.clone();
    assert_eq!(&*arena.into_nodes(), expected);
}
