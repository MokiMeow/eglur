use super::test_support::root;
use super::*;
use eglur_diagnostics::{DiagnosticFailure, DiagnosticSnapshot, DiagnosticStoreError};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Work {
    pub(super) inputs: usize,
    pub(super) completed: usize,
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

pub(super) fn reset() {
    WORK.set(Work::default());
}

pub(super) fn work() -> Work {
    WORK.get()
}

fn fold(inputs: impl IntoIterator<Item = Outcome<()>>) -> Result<Outcome<()>, BodyFlow> {
    reset();
    let mut state = UnitOutcomeFold::Known;
    for input in inputs {
        state = state.include(input)?;
    }
    Ok(state.finish())
}

fn union(inputs: &[Poison], indices: impl IntoIterator<Item = usize>) -> Poison {
    let mut pending = eglur_diagnostics::PoisonAccumulator::new();
    for index in indices {
        pending.include(&inputs[index]).unwrap();
    }
    pending.finish().unwrap()
}

fn selected(store: &mut DiagnosticStore, poison: Poison, all: &Poison) -> Vec<String> {
    store
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    store.select_preceding_roots(&poison, all).unwrap();
    let snapshot = std::mem::replace(store, DiagnosticStore::new()).finalize(None);
    assert!(matches!(snapshot, DiagnosticSnapshot::Rejected(_)));
    snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str().to_owned())
        .collect()
}

fn state(rank: usize, poison: &Poison) -> Outcome<()> {
    match rank {
        0 => Outcome::Known(()),
        1 => Outcome::NotApplicable,
        2 => Outcome::Deferred,
        3 => Outcome::Unperformed,
        _ => Outcome::Poisoned(poison.clone()),
    }
}

fn rank(outcome: &Outcome<()>) -> usize {
    match outcome {
        Outcome::Known(()) => 0,
        Outcome::NotApplicable => 1,
        Outcome::Deferred => 2,
        Outcome::Unperformed => 3,
        Outcome::Poisoned(_) => 4,
    }
}

#[test]
fn unit_fold_completes_once_for_distinct_repeated_overlapping_wide_and_reversed_inputs() {
    for count in [8_usize, 32, 128] {
        for mode in ["distinct", "repeated", "overlap", "wide", "reversed"] {
            let mut store = DiagnosticStore::new();
            let inputs = (0..count)
                .map(|index| root(&mut store, index))
                .collect::<Vec<_>>();
            let all = union(&inputs, 0..count);
            let left = union(&inputs, 0..count / 2 + 1);
            let right = union(&inputs, count / 2..count);
            let outcomes = (0..count).map(|index| {
                Outcome::Poisoned(match mode {
                    "repeated" => inputs[0].clone(),
                    "wide" => all.clone(),
                    "overlap" if index % 2 == 0 => left.clone(),
                    "overlap" => right.clone(),
                    "reversed" => inputs[count - 1 - index].clone(),
                    _ => inputs[index].clone(),
                })
            });
            let Outcome::Poisoned(poison) = fold(outcomes).unwrap() else {
                panic!("poisoned fold expected")
            };
            assert_eq!(
                work(),
                Work {
                    inputs: count,
                    completed: 1
                }
            );
            assert_eq!(
                selected(&mut store, poison, &all),
                (0..if mode == "repeated" { 1 } else { count })
                    .map(|i| format!("ROOT_{i:03}"))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn unit_fold_preserves_all_twenty_five_ordered_state_pairs() {
    for left in 0..5 {
        for right in 0..5 {
            let mut store = DiagnosticStore::new();
            let roots = [root(&mut store, 0), root(&mut store, 1)];
            let all = union(&roots, 0..2);
            let outcome = fold([state(left, &roots[0]), state(right, &roots[1])]).unwrap();
            assert_eq!(rank(&outcome), left.max(right));
            let inputs = usize::from(left == 4) + usize::from(right == 4);
            assert_eq!(
                work(),
                Work {
                    inputs,
                    completed: usize::from(inputs != 0)
                }
            );
            if let Outcome::Poisoned(poison) = outcome {
                let expected = (0..2)
                    .filter(|&i| if i == 0 { left } else { right } == 4)
                    .map(|i| format!("ROOT_{i:03}"))
                    .collect::<Vec<_>>();
                assert_eq!(selected(&mut store, poison, &all), expected);
            }
        }
    }
}

#[test]
fn unit_fold_retains_later_poison_after_every_partial_state() {
    for partial in 0..4 {
        let mut store = DiagnosticStore::new();
        let roots = [root(&mut store, 0), root(&mut store, 1)];
        let all = union(&roots, 0..2);
        let Outcome::Poisoned(poison) = fold([
            state(4, &roots[0]),
            state(partial, &roots[0]),
            state(4, &roots[1]),
            state(partial, &roots[1]),
        ])
        .unwrap() else {
            panic!("poison dominates partial state")
        };
        assert_eq!(
            work(),
            Work {
                inputs: 2,
                completed: 1
            }
        );
        assert_eq!(selected(&mut store, poison, &all), ["ROOT_000", "ROOT_001"]);
    }
}

#[test]
fn empty_and_nonpoison_unit_folds_do_not_complete_provenance() {
    let mut store = DiagnosticStore::new();
    let poison = root(&mut store, 0);
    assert_eq!(rank(&fold([]).unwrap()), 0);
    assert_eq!(work(), Work::default());
    for count in [1, 8, 32, 128] {
        for expected in 0..4 {
            let outcome = fold((0..count).map(|i| state(i.min(expected), &poison))).unwrap();
            assert_eq!(rank(&outcome), (count - 1).min(expected));
            assert_eq!(work(), Work::default());
        }
    }
}

#[test]
fn foreign_unit_inputs_refuse_the_first_mixed_pool_without_completion() {
    for count in [8_usize, 32, 128] {
        for foreign_first in [true, false] {
            let mut store = DiagnosticStore::new();
            let mut foreign = DiagnosticStore::new();
            let own = root(&mut store, 0);
            let other = root(&mut foreign, 1);
            let result = fold((0..count).map(|i| {
                Outcome::Poisoned(if i == if foreign_first { 0 } else { count - 1 } {
                    other.clone()
                } else {
                    own.clone()
                })
            }));
            assert!(matches!(
                result,
                Err(BodyFlow::Internal(BodyTypingError::DiagnosticStore(
                    DiagnosticStoreError::ForeignPoison
                )))
            ));
            assert_eq!(
                work(),
                Work {
                    inputs: if foreign_first { 2 } else { count },
                    completed: 0
                }
            );
            assert!(
                store
                    .record_failure(DiagnosticFailure::Derivative(own))
                    .is_ok()
            );
            assert!(
                foreign
                    .record_failure(DiagnosticFailure::Derivative(other))
                    .is_ok()
            );
        }
    }
}

#[test]
fn lone_unit_poison_selects_its_input_pool_not_a_checker_pool() {
    let mut store = DiagnosticStore::new();
    let mut foreign = DiagnosticStore::new();
    let input = root(&mut foreign, 0);
    let Outcome::Poisoned(poison) = fold([
        Outcome::Known(()),
        Outcome::Poisoned(input),
        Outcome::Unperformed,
    ])
    .unwrap() else {
        panic!("poisoned fold expected")
    };
    assert_eq!(
        work(),
        Work {
            inputs: 1,
            completed: 1
        }
    );
    assert!(matches!(
        store.record_failure(DiagnosticFailure::Derivative(poison.clone())),
        Err(DiagnosticStoreError::ForeignPoison)
    ));
    assert!(
        foreign
            .record_failure(DiagnosticFailure::Derivative(poison))
            .is_ok()
    );
}
