use std::collections::BTreeMap;

use super::*;

fn insert(index: &mut BoundIndex, root: &mut Option<BoundLink>, key: usize, holder: usize) {
    let BoundLookup::Vacant(insertion) = index.lookup(*root, key) else {
        panic!("expected a new original definition")
    };
    assert!(index.inspected.get() <= 2 * usize::BITS as usize);
    let reservation = index.reservation(&insertion).unwrap();
    index.install(root, insertion, holder, reservation);
}

fn check(index: &BoundIndex, root: Option<BoundLink>, expected: &BTreeMap<usize, usize>) {
    for (&key, &holder) in expected {
        assert!(matches!(index.lookup(root, key), BoundLookup::Existing(found) if found == holder));
        assert!(index.inspected.get() <= 2 * usize::BITS as usize);
    }
    let mut pending = root
        .into_iter()
        .map(|link| (None, link))
        .collect::<Vec<_>>();
    while let Some((previous, link)) = pending.pop() {
        if let BoundLink::Branch(id) = link {
            let branch = index.branches[id];
            assert!(previous.is_none_or(|bit| bit < branch.bit));
            assert!(branch.bit < usize::BITS);
            pending.extend(branch.children.map(|child| (Some(branch.bit), child)));
        }
    }
}

#[test]
fn insertion_and_replay_match_a_map_under_adversarial_orders() {
    let mut keys = (0..256).collect::<Vec<usize>>();
    keys.extend((0..usize::BITS).map(|bit| 1_usize << bit));
    keys.extend([usize::MAX, usize::MAX - 1]);
    keys.sort_unstable();
    keys.dedup();
    for order in 0..4 {
        match order {
            1 => keys.reverse(),
            2 => keys.sort_by_key(|key| key.reverse_bits()),
            3 => keys.sort_by_key(|key| key.wrapping_mul(0x9e37_79b9)),
            _ => {}
        }
        let mut index = BoundIndex::default();
        let mut root = None;
        let mut expected = BTreeMap::new();
        for (holder, &key) in keys.iter().enumerate() {
            insert(&mut index, &mut root, key, holder);
            expected.insert(key, holder);
            check(&index, root, &expected);
        }
        assert_eq!(index.branches.len(), expected.len() - 1);
        assert!(matches!(index.lookup(root, 513), BoundLookup::Vacant(_)));
        assert!(index.inspected.get() <= 2 * usize::BITS as usize);
    }
}

#[test]
fn separate_source_sites_can_share_branch_storage_without_sharing_entries() {
    let mut index = BoundIndex::default();
    let mut roots = [None; 3];
    for key in [0, 1, usize::MAX, 32, 64] {
        for (site, root) in roots.iter_mut().enumerate() {
            insert(&mut index, root, key, key.wrapping_add(site));
        }
    }
    for (site, root) in roots.into_iter().enumerate() {
        for key in [0, 1, usize::MAX, 32, 64] {
            assert!(
                matches!(index.lookup(root, key), BoundLookup::Existing(found) if found == key.wrapping_add(site))
            );
        }
    }
}

#[test]
fn preparing_a_missing_key_does_not_mutate_the_existing_index() {
    let mut index = BoundIndex::default();
    let mut root = None;
    insert(&mut index, &mut root, 0, 5);
    for key in [1, usize::MAX, 1_usize << (usize::BITS - 1)] {
        let BoundLookup::Vacant(insertion) = index.lookup(root, key) else {
            panic!("missing key")
        };
        let _reservation = index.reservation(&insertion).unwrap();
        assert!(index.branches.is_empty());
        assert_eq!(root, Some(BoundLink::Leaf { key: 0, holder: 5 }));
    }
}
