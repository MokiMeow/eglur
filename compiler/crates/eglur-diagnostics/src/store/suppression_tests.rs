use std::cell::Cell;

use eglur_source::{ByteOffset, ByteRange, FileId, FileSpan};

use super::*;
use crate::{DiagnosticPhase, PrimarySpan, SourceLabel};

thread_local! {
    static COMPARISONS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn observe_comparison() {
    COMPARISONS.update(|count| count + 1);
}

fn span(start: usize, end: usize) -> SourceSpan {
    SourceSpan::byte_only(
        SourceLabel::new("main.eglur").unwrap(),
        FileSpan::new(
            FileId::new(1),
            ByteRange::new(ByteOffset::new(start as u32), ByteOffset::new(end as u32)).unwrap(),
        ),
    )
}

fn roots(store: &mut DiagnosticStore, count: usize) -> Vec<Poison> {
    (0..count)
        .map(|index| {
            let diagnostic = Diagnostic::new(
                DiagnosticCode::new(format!("ROOT_{index:03}")).unwrap(),
                Severity::Error,
                DiagnosticPhase::new("type").unwrap(),
                "independent failure",
                PrimarySpan::Source(span(count - index, count - index + 1)),
            );
            let root = DiagnosticRoot::source(diagnostic).unwrap();
            store.record_failure(DiagnosticFailure::Root(root)).unwrap()
        })
        .collect()
}

fn aggregate(inputs: &[Poison], ids: impl IntoIterator<Item = usize>) -> Poison {
    let mut pending = PoisonAccumulator::new();
    for id in ids {
        pending.include(&inputs[id]).unwrap();
    }
    pending.finish().unwrap()
}

fn comparison_bound(count: usize) -> usize {
    count.ilog2() as usize + 2
}

#[test]
fn all_producers_preserve_sorted_membership_independent_of_source_order() {
    let mut store = DiagnosticStore::new();
    let checkpoint = store.checkpoint();
    let inputs = roots(&mut store, 9);
    let merged = inputs
        .iter()
        .rev()
        .try_fold(inputs[8].clone(), |left, right| left.merge(right))
        .unwrap();
    let accumulated = aggregate(&inputs, (0..9).rev());
    let discovered = store
        .roots_since_within(&checkpoint, &span(0, 11))
        .unwrap()
        .unwrap();

    for poison in [&inputs[4], &merged, &accumulated, &discovered] {
        assert!(poison.roots().windows(2).all(|pair| pair[0] < pair[1]));
        for root in 0..11 {
            COMPARISONS.set(0);
            assert_eq!(poison.contains_root(&root), poison.roots().contains(&root));
            assert!(COMPARISONS.get() > 0);
            assert!(COMPARISONS.get() <= comparison_bound(poison.roots().len()));
        }
    }
    assert_eq!(merged.roots(), accumulated.roots());
    assert_eq!(merged.roots(), discovered.roots());
}

#[test]
fn balanced_and_unbalanced_queries_have_logarithmic_membership_work() {
    for count in [8, 9, 32, 33, 128, 129, 257] {
        let mut store = DiagnosticStore::new();
        let inputs = roots(&mut store, 2 * count + 1);
        let wide = aggregate(&inputs, (0..count).rev());
        for later_ids in [(count..2 * count).collect(), vec![count - 1], vec![count]] {
            let later = aggregate(&inputs, later_ids.clone());
            COMPARISONS.set(0);
            store.select_preceding_roots(&wide, &later).unwrap();
            let comparisons = COMPARISONS.get();
            assert!(comparisons >= later_ids.len());
            assert!(comparisons <= later_ids.len() * comparison_bound(count));
        }

        let single = &inputs[2 * count];
        let later = aggregate(&inputs, 0..count);
        COMPARISONS.set(0);
        store.select_preceding_roots(single, &later).unwrap();
        assert_eq!(COMPARISONS.get(), count);
    }
}

#[test]
fn every_small_subset_pair_suppresses_only_later_minus_preceding() {
    for preceding_mask in 1..32 {
        for later_mask in 1..32 {
            let mut store = DiagnosticStore::new();
            let inputs = roots(&mut store, 5);
            let selected = |mask| (0..5).filter(move |index| mask & (1 << index) != 0);
            let preceding = aggregate(&inputs, selected(preceding_mask).rev());
            let later = aggregate(&inputs, selected(later_mask));
            let before = (preceding.roots().to_vec(), later.roots().to_vec());
            let expected = selected(later_mask)
                .filter(|index| preceding_mask & (1 << index) == 0)
                .collect::<HashSet<_>>();

            store.select_preceding_roots(&preceding, &later).unwrap();
            assert_eq!(store.superseded, expected);
            assert_eq!(preceding.roots(), before.0);
            assert_eq!(later.roots(), before.1);

            let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(None) else {
                panic!("preceding roots remain retained");
            };
            let expected_codes = (0..5)
                .rev()
                .filter(|index| !expected.contains(index))
                .map(|index| format!("ROOT_{index:03}"))
                .collect::<Vec<_>>();
            assert_eq!(
                snapshot
                    .diagnostics()
                    .iter()
                    .map(|diagnostic| diagnostic.code().as_str())
                    .collect::<Vec<_>>(),
                expected_codes
            );
        }
    }
}

#[test]
fn partial_expiry_preserves_shared_membership_and_complete_expiry_is_atomic() {
    for expired in [2, 4] {
        let mut store = DiagnosticStore::new();
        let inputs = roots(&mut store, 9);
        let preceding = aggregate(&inputs, 0..4);
        let later = aggregate(&inputs, 2..6);
        store
            .select_preceding_roots(&inputs[8], &aggregate(&inputs, 0..expired))
            .unwrap();
        let before = store.superseded.clone();
        let checkpoint = store.checkpoint();
        COMPARISONS.set(0);

        let result = store.select_preceding_roots(&preceding, &later);
        if expired == 4 {
            assert_eq!(result, Err(DiagnosticStoreError::NoPrecedingRoot));
            assert_eq!(store.superseded, before);
            assert_eq!(COMPARISONS.get(), 0);
        } else {
            assert_eq!(result, Ok(()));
            assert_eq!(store.superseded, HashSet::from([0, 1, 4, 5]));
            assert!(COMPARISONS.get() > 0);
        }

        let derivative = store
            .record_failure(DiagnosticFailure::Derivative(preceding.clone()))
            .unwrap();
        assert_eq!(derivative.roots(), preceding.roots());
        assert_eq!(preceding.roots(), [0, 1, 2, 3]);
        assert!(
            store
                .roots_since_within(&checkpoint, &span(0, 11))
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn foreign_inputs_fail_before_membership_mutation_or_expiry_resolution() {
    for (foreign_preceding, foreign_later, expired) in [
        (true, false, false),
        (false, true, false),
        (true, true, false),
        (false, true, true),
    ] {
        let mut store = DiagnosticStore::new();
        let local = roots(&mut store, 3);
        let mut other = DiagnosticStore::new();
        let foreign = roots(&mut other, 3);
        if expired {
            store.select_preceding_roots(&local[2], &local[0]).unwrap();
        }
        let preceding = if foreign_preceding {
            &foreign[0]
        } else {
            &local[0]
        };
        let later = if foreign_later {
            &foreign[1]
        } else {
            &local[1]
        };
        let before = store.superseded.clone();
        COMPARISONS.set(0);

        assert_eq!(
            store.select_preceding_roots(preceding, later),
            Err(DiagnosticStoreError::ForeignPoison)
        );
        assert_eq!(store.superseded, before);
        assert_eq!(COMPARISONS.get(), 0);
    }
}

#[test]
fn suppression_preserves_checkpoint_discovery_and_limited_snapshot_order() {
    let mut store = DiagnosticStore::new();
    let checkpoint = store.checkpoint();
    let inputs = roots(&mut store, 9);
    let preceding = aggregate(&inputs, [6, 2]);
    let later = aggregate(&inputs, [0, 1, 2, 3, 4]);
    store.select_preceding_roots(&preceding, &later).unwrap();

    let discovered = store
        .roots_since_within(&checkpoint, &span(0, 11))
        .unwrap()
        .unwrap();
    assert_eq!(discovered.roots(), (0..9).collect::<Vec<_>>());
    let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(ErrorLimit::new(2)) else {
        panic!("preceding roots remain retained");
    };
    assert_eq!(snapshot.first_error().code().as_str(), "ROOT_008");
    assert_eq!(snapshot.omitted_error_count(), 3);
    assert_eq!(
        snapshot
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code().as_str())
            .collect::<Vec<_>>(),
        ["ROOT_008", "ROOT_007"]
    );
}
