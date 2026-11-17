use super::*;
use crate::body_types::checker::test_support::root;
use eglur_diagnostics::{
    DiagnosticFailure, DiagnosticSnapshot, DiagnosticStore, DiagnosticStoreError,
};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::body_types::checker) struct Work {
    pub(in crate::body_types::checker) inputs: usize,
    pub(in crate::body_types::checker) completed: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { inputs: 0, completed: 0 }) };
}

pub(in crate::body_types::checker) enum Step {
    Input,
    Complete,
}

pub(in crate::body_types::checker) fn observe(step: Step) {
    WORK.update(|mut work| {
        match step {
            Step::Input => work.inputs += 1,
            Step::Complete => work.completed += 1,
        }
        work
    });
}

pub(in crate::body_types::checker) fn reset() {
    WORK.set(Work::default());
}

pub(in crate::body_types::checker) fn work() -> Work {
    WORK.get()
}

pub(in crate::body_types::checker) fn union(
    inputs: &[Poison],
    indices: impl IntoIterator<Item = usize>,
) -> Poison {
    let mut pending = eglur_diagnostics::PoisonAccumulator::new();
    for index in indices {
        pending.include(&inputs[index]).unwrap();
    }
    pending.finish().unwrap()
}

pub(in crate::body_types::checker) fn selected(
    store: &mut DiagnosticStore,
    poison: &Poison,
    all: &Poison,
) -> Vec<String> {
    store
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    store.select_preceding_roots(poison, all).unwrap();
    let snapshot = std::mem::replace(store, DiagnosticStore::new()).finalize(None);
    assert!(matches!(snapshot, DiagnosticSnapshot::Rejected(_)));
    snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str().to_owned())
        .collect()
}

#[test]
fn pending_poison_completes_distinct_shared_overlapping_wide_and_reverse_roots_once() {
    for count in [8_usize, 32, 128] {
        for mode in ["distinct", "repeated", "overlap", "wide", "reversed"] {
            let mut store = DiagnosticStore::new();
            let roots = (0..count).map(|i| root(&mut store, i)).collect::<Vec<_>>();
            let all = union(&roots, 0..count);
            let left = union(&roots, 0..count / 2 + 1);
            let right = union(&roots, count / 2..count);
            reset();
            let mut pending = PendingPoison::default();
            for i in 0..count {
                let input = match mode {
                    "repeated" => &roots[0],
                    "overlap" if i % 2 == 0 => &left,
                    "overlap" => &right,
                    "wide" => &all,
                    "reversed" => &roots[count - 1 - i],
                    _ => &roots[i],
                };
                pending.include(input).unwrap();
                assert_eq!(work().completed, 0);
            }
            let poison = pending.take().unwrap();
            assert_eq!(
                work(),
                Work {
                    inputs: count,
                    completed: 1
                }
            );
            assert!(pending.take().is_none());
            assert_eq!(work().completed, 1);
            assert_eq!(
                selected(&mut store, &poison, &all),
                (0..if mode == "repeated" { 1 } else { count })
                    .map(|i| format!("ROOT_{i:03}"))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn empty_pending_poison_reads_remain_reusable_between_inference_passes() {
    let mut store = DiagnosticStore::new();
    let root = root(&mut store, 0);
    let mut pending = PendingPoison::default();
    reset();
    for _ in 0..128 {
        assert!(pending.take().is_none());
    }
    assert_eq!(work(), Work::default());
    pending.include(&root).unwrap();
    let poison = pending.take().unwrap();
    assert_eq!(
        work(),
        Work {
            inputs: 1,
            completed: 1
        }
    );
    pending.include(&poison).unwrap();
    assert!(pending.take().is_some());
    assert_eq!(
        work(),
        Work {
            inputs: 2,
            completed: 2
        }
    );
}

#[test]
fn foreign_pending_poison_refusal_clears_the_slot_in_both_pool_orders() {
    for foreign_first in [false, true] {
        let mut store = DiagnosticStore::new();
        let mut foreign = DiagnosticStore::new();
        let own = root(&mut store, 0);
        let other = root(&mut foreign, 1);
        let (first, second) = if foreign_first {
            (&other, &own)
        } else {
            (&own, &other)
        };
        reset();
        let mut pending = PendingPoison::default();
        pending.include(first).unwrap();
        assert!(matches!(
            pending.include(second),
            Err(BodyTypingError::DiagnosticStore(
                DiagnosticStoreError::ForeignPoison
            ))
        ));
        assert!(pending.take().is_none());
        assert_eq!(
            work(),
            Work {
                inputs: 2,
                completed: 0
            }
        );
        pending.include(&own).unwrap();
        let poison = pending.take().unwrap();
        assert_eq!(
            work(),
            Work {
                inputs: 3,
                completed: 1
            }
        );
        assert_eq!(selected(&mut store, &poison, &own), ["ROOT_000"]);
    }
}

#[test]
fn completed_singleton_retains_the_first_inputs_pool_without_checker_authorization() {
    let mut foreign = DiagnosticStore::new();
    let root = root(&mut foreign, 3);
    reset();
    let mut pending = PendingPoison::default();
    pending.include(&root).unwrap();
    let poison = pending.take().unwrap();
    assert_eq!(selected(&mut foreign, &poison, &root), ["ROOT_003"]);
    assert_eq!(
        work(),
        Work {
            inputs: 1,
            completed: 1
        }
    );
}
