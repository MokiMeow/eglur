use std::cell::Cell;

use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticSnapshot, DiagnosticStore, PrimarySpan, Severity,
};

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

fn aggregate(inputs: &[Poison], ids: impl IntoIterator<Item = usize>) -> Poison {
    let mut pending = PoisonAccumulator::new();
    for index in ids {
        pending.include(&inputs[index]).unwrap();
    }
    pending.finish().unwrap()
}

fn complete(state: EffectBodyState) -> Poison {
    let EffectBodyState::Poisoned(pending) = state else {
        panic!("poisoned body state expected");
    };
    EffectBodyState::complete_provenance(pending)
}

fn assert_roots(
    mut store: DiagnosticStore,
    inputs: &[Poison],
    poison: &Poison,
    expected: impl IntoIterator<Item = usize>,
) {
    store
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    store
        .select_preceding_roots(poison, &aggregate(inputs, 0..inputs.len()))
        .unwrap();
    let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(None) else {
        panic!("nonempty provenance retains its roots");
    };
    assert_eq!(
        snapshot
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code().as_str())
            .collect::<Vec<_>>(),
        expected
            .into_iter()
            .map(|index| format!("ROOT_{index:03}"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn unblocked_and_unperformed_states_do_not_complete_provenance() {
    WORK.set(Work::default());
    let mut state = EffectBodyState::Unblocked;
    state.mark_unperformed();
    assert!(matches!(state, EffectBodyState::Unperformed));
    state.mark_unperformed();
    assert!(matches!(state, EffectBodyState::Unperformed));
    assert_eq!(WORK.get(), Work::default());
}

#[test]
fn distinct_contributions_complete_only_at_publication_in_either_order() {
    for count in [8, 32, 128] {
        for reverse in [false, true] {
            let mut store = DiagnosticStore::new();
            let inputs = roots(&mut store, count);
            let mut state = EffectBodyState::Unblocked;
            WORK.set(Work::default());
            for offset in 0..count {
                let index = if reverse { count - offset - 1 } else { offset };
                state.include_poison(&inputs[index]).unwrap();
                assert!(matches!(state, EffectBodyState::Poisoned(_)));
                assert_eq!(WORK.get().completed, 0);
            }
            let poison = complete(state);
            assert_eq!(
                WORK.get(),
                Work {
                    inputs: count,
                    completed: 1
                }
            );
            assert_roots(store, &inputs, &poison, 0..count);
        }
    }
}

#[test]
fn every_small_subset_pair_retains_the_complete_union() {
    for left_mask in 1..32 {
        for right_mask in 1..32 {
            let mut store = DiagnosticStore::new();
            let inputs = roots(&mut store, 5);
            let selected = |mask| (0..5).filter(move |index| mask & (1 << index) != 0);
            let left = aggregate(&inputs, selected(left_mask).rev());
            let right = aggregate(&inputs, selected(right_mask));
            let mut state = EffectBodyState::Unblocked;
            WORK.set(Work::default());
            state.include_poison(&left).unwrap();
            state.include_poison(&right).unwrap();
            assert_eq!(
                WORK.get(),
                Work {
                    inputs: 2,
                    completed: 0
                }
            );
            let poison = complete(state);
            assert_eq!(
                WORK.get(),
                Work {
                    inputs: 2,
                    completed: 1
                }
            );
            assert_roots(store, &inputs, &poison, selected(left_mask | right_mask));
        }
    }
}

#[test]
fn repeated_shared_and_overlapping_inputs_dominate_unperformed() {
    for count in [8, 32, 128] {
        for mode in 0..3 {
            let mut store = DiagnosticStore::new();
            let inputs = roots(&mut store, count);
            let all = aggregate(&inputs, 0..count);
            let left = aggregate(&inputs, 0..count / 2 + 1);
            let right = aggregate(&inputs, count / 2..count);
            let mut state = EffectBodyState::Unblocked;
            state.mark_unperformed();
            WORK.set(Work::default());
            for index in 0..count {
                let input = match mode {
                    0 => &inputs[0],
                    1 => &all,
                    _ if index % 2 == 0 => &left,
                    _ => &right,
                };
                state.include_poison(input).unwrap();
                state.mark_unperformed();
                assert!(matches!(state, EffectBodyState::Poisoned(_)));
                assert_eq!(WORK.get().completed, 0);
            }
            let poison = complete(state);
            assert_eq!(
                WORK.get(),
                Work {
                    inputs: count,
                    completed: 1
                }
            );
            let expected = 0..if mode == 0 { 1 } else { count };
            assert_roots(store, &inputs, &poison, expected);
        }
    }
}

#[test]
fn foreign_contribution_consumes_prior_state_and_allows_a_new_store() {
    let mut left_store = DiagnosticStore::new();
    let left = roots(&mut left_store, 3);
    let mut right_store = DiagnosticStore::new();
    let right = roots(&mut right_store, 3);
    let mut state = EffectBodyState::Unblocked;
    WORK.set(Work::default());
    state.include_poison(&left[0]).unwrap();
    assert!(matches!(
        state.include_poison(&right[1]),
        Err(EffectBodyError::DiagnosticStore(
            DiagnosticStoreError::ForeignPoison
        ))
    ));
    assert!(matches!(state, EffectBodyState::Unblocked));
    assert_eq!(
        WORK.get(),
        Work {
            inputs: 2,
            completed: 0
        }
    );

    state.mark_unperformed();
    state.include_poison(&right[2]).unwrap();
    let poison = complete(state);
    assert_eq!(
        WORK.get(),
        Work {
            inputs: 3,
            completed: 1
        }
    );
    assert_eq!(
        left_store
            .record_failure(DiagnosticFailure::Derivative(poison.clone()))
            .unwrap_err(),
        DiagnosticStoreError::ForeignPoison
    );
    assert_roots(right_store, &right, &poison, [2]);
}

#[test]
#[should_panic(expected = "poisoned effect-body state includes nonempty provenance")]
fn empty_pending_data_cannot_publish_a_poisoned_fact() {
    EffectBodyState::complete_provenance(PoisonAccumulator::new());
}
