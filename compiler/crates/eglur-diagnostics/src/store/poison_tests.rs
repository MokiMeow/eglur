use std::cell::Cell;

use eglur_source::{ByteOffset, ByteRange, FileId, FileSpan};

use super::*;
use crate::{BoundaryRootKey, DiagnosticPhase, PrimarySpan, SourceLabel};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    inputs: usize,
    pool_checks: usize,
    foreign: usize,
    roots: usize,
    finishes: usize,
    prepared: usize,
}

thread_local! {
    static WORK: Cell<Work> = Cell::new(Work::default());
}

pub(super) enum Step {
    Input,
    PoolCheck,
    Foreign,
    Root,
    Finish(usize),
}

pub(super) fn observe(step: Step) {
    WORK.update(|mut work| {
        match step {
            Step::Input => work.inputs += 1,
            Step::PoolCheck => work.pool_checks += 1,
            Step::Foreign => work.foreign += 1,
            Step::Root => work.roots += 1,
            Step::Finish(count) => {
                work.finishes += 1;
                work.prepared += count;
            }
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

fn subset(roots: &[Poison], mask: usize) -> Poison {
    let mut selected = roots
        .iter()
        .enumerate()
        .filter(|(index, _)| mask & (1 << index) != 0)
        .map(|(_, root)| root);
    let mut result = selected.next().unwrap().clone();
    for root in selected {
        result = result.merge(root).unwrap();
    }
    result
}

#[test]
fn empty_accumulation_cannot_create_poison() {
    WORK.set(Work::default());
    assert!(PoisonAccumulator::new().finish().is_none());
    assert!(PoisonAccumulator::default().finish().is_none());
    assert_eq!(WORK.get(), Work::default());
}

#[test]
fn distinct_inputs_materialize_only_the_final_union_in_either_order() {
    for count in [8, 32, 128] {
        let mut store = DiagnosticStore::new();
        let inputs = roots(&mut store, count);
        for reverse in [false, true] {
            WORK.set(Work::default());
            let mut accumulated = PoisonAccumulator::new();
            for offset in 0..count {
                let index = if reverse { count - offset - 1 } else { offset };
                accumulated.include(&inputs[index]).unwrap();
                assert_eq!(accumulated.roots.len(), offset + 1);
                assert_eq!(WORK.get().prepared, 0);
            }
            let result = accumulated.finish().unwrap();
            assert_eq!(result.roots.as_ref(), (0..count).collect::<Vec<_>>());
            assert!(Arc::ptr_eq(&result.store, &store.identity));
            assert_eq!(
                WORK.get(),
                Work {
                    inputs: count,
                    pool_checks: count - 1,
                    roots: count,
                    finishes: 1,
                    prepared: count,
                    ..Work::default()
                }
            );
            for (index, input) in inputs.iter().enumerate() {
                assert_eq!(input.roots.as_ref(), [index]);
            }
        }
    }
}

#[test]
fn repeated_provenance_does_not_retain_duplicate_pending_rows() {
    for count in [8, 32, 128] {
        let mut store = DiagnosticStore::new();
        let inputs = roots(&mut store, count);
        let complete = inputs
            .iter()
            .skip(1)
            .try_fold(inputs[0].clone(), |left, right| left.merge(right))
            .unwrap();

        for input in [&inputs[0], &complete] {
            WORK.set(Work::default());
            let mut accumulated = PoisonAccumulator::new();
            for _ in 0..count {
                accumulated.include(input).unwrap();
                assert_eq!(accumulated.roots.len(), input.roots.len());
                assert_eq!(WORK.get().prepared, 0);
            }
            let result = accumulated.finish().unwrap();
            assert_eq!(result.roots, input.roots);
            assert_eq!(
                WORK.get(),
                Work {
                    inputs: count,
                    pool_checks: count - 1,
                    roots: count * input.roots.len(),
                    finishes: 1,
                    prepared: input.roots.len(),
                    ..Work::default()
                }
            );
        }
    }
}

#[test]
fn every_small_pair_of_subsets_matches_the_existing_pairwise_union() {
    let mut store = DiagnosticStore::new();
    let inputs = roots(&mut store, 5);
    for left in 1..32 {
        for right in 1..32 {
            let left = subset(&inputs, left);
            let right = subset(&inputs, right);
            let expected = left.merge(&right).unwrap();
            let mut accumulated = PoisonAccumulator::new();
            accumulated.include(&left).unwrap();
            accumulated.include(&right).unwrap();
            let result = accumulated.finish().unwrap();
            assert_eq!(result.roots, expected.roots);
            assert!(Arc::ptr_eq(&result.store, &expected.store));
        }
    }
}

#[test]
fn foreign_input_is_rejected_before_pending_state_changes() {
    let mut local = DiagnosticStore::new();
    let mut other = DiagnosticStore::new();
    let inputs = roots(&mut local, 3);
    let foreign = roots(&mut other, 3);
    let mut accumulated = PoisonAccumulator::new();
    accumulated.include(&inputs[0]).unwrap();
    accumulated.include(&inputs[2]).unwrap();
    let before = accumulated.roots.clone();

    WORK.set(Work::default());
    assert_eq!(
        accumulated.include(&subset(&foreign, 7)),
        Err(DiagnosticStoreError::ForeignPoison)
    );
    assert_eq!(accumulated.roots, before);
    assert!(Arc::ptr_eq(
        accumulated.store.as_ref().unwrap(),
        &local.identity
    ));
    assert_eq!(
        WORK.get(),
        Work {
            inputs: 1,
            pool_checks: 1,
            foreign: 1,
            ..Work::default()
        }
    );

    accumulated.include(&inputs[1]).unwrap();
    let result = accumulated.finish().unwrap();
    assert_eq!(result.roots.as_ref(), [0, 1, 2]);
    local
        .record_failure(DiagnosticFailure::Derivative(result.clone()))
        .unwrap();
    assert_eq!(
        other
            .record_failure(DiagnosticFailure::Derivative(result))
            .unwrap_err(),
        DiagnosticStoreError::ForeignPoison
    );
}

#[test]
fn first_input_establishes_identity_without_rebinding_previous_results() {
    let mut first = DiagnosticStore::new();
    let mut second = DiagnosticStore::new();
    let first_root = roots(&mut first, 1).pop().unwrap();
    let second_root = roots(&mut second, 1).pop().unwrap();
    assert_eq!(first_root.roots, second_root.roots);

    let mut accumulated = PoisonAccumulator::new();
    accumulated.include(&first_root).unwrap();
    let first_result = accumulated.finish().unwrap();
    let mut accumulated = PoisonAccumulator::new();
    accumulated.include(&second_root).unwrap();
    assert_eq!(
        accumulated.include(&first_result),
        Err(DiagnosticStoreError::ForeignPoison)
    );
    let second_result = accumulated.finish().unwrap();
    assert!(Arc::ptr_eq(&first_result.store, &first.identity));
    assert!(Arc::ptr_eq(&second_result.store, &second.identity));

    drop(first);
    assert_eq!(first_result.merge(&first_root).unwrap().roots.as_ref(), [0]);
    assert_eq!(
        first_result.merge(&second_result).unwrap_err(),
        DiagnosticStoreError::ForeignPoison
    );
}

fn source_root(store: &mut DiagnosticStore, position: u32) -> Poison {
    let span = SourceSpan::byte_only(
        SourceLabel::new("main.eglur").unwrap(),
        FileSpan::new(
            FileId::new(1),
            ByteRange::new(ByteOffset::new(position), ByteOffset::new(position + 1)).unwrap(),
        ),
    );
    let diagnostic = Diagnostic::new(
        DiagnosticCode::new("NAME_NOT_FOUND").unwrap(),
        Severity::Error,
        DiagnosticPhase::new("resolve").unwrap(),
        "missing name",
        PrimarySpan::Source(span),
    );
    store
        .record_failure(DiagnosticFailure::Root(
            DiagnosticRoot::source(diagnostic).unwrap(),
        ))
        .unwrap()
}

#[test]
fn suppression_does_not_change_accumulated_or_prior_provenance() {
    let mut store = DiagnosticStore::new();
    let first = source_root(&mut store, 20);
    let checkpoint = store.checkpoint();
    let second = source_root(&mut store, 5);
    let third = source_root(&mut store, 10);
    let expected = first.merge(&second).unwrap().merge(&third).unwrap();
    let prior = expected.clone();

    let mut accumulated = PoisonAccumulator::new();
    accumulated.include(&third).unwrap();
    accumulated.include(&first).unwrap();
    store.select_preceding_roots(&first, &second).unwrap();
    accumulated.include(&second).unwrap();
    let result = accumulated.finish().unwrap();
    assert_eq!(result.roots, expected.roots);
    assert_eq!(prior.roots.as_ref(), [0, 1, 2]);
    store
        .record_failure(DiagnosticFailure::Derivative(result))
        .unwrap();

    let construct = SourceSpan::byte_only(
        SourceLabel::new("main.eglur").unwrap(),
        FileSpan::new(
            FileId::new(1),
            ByteRange::new(ByteOffset::new(0), ByteOffset::new(30)).unwrap(),
        ),
    );
    assert_eq!(
        store
            .roots_since_within(&checkpoint, &construct)
            .unwrap()
            .unwrap()
            .roots
            .as_ref(),
        [1, 2]
    );
    let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(ErrorLimit::new(1)) else {
        panic!("independent retained roots must reject compilation");
    };
    assert_eq!(snapshot.diagnostics().len(), 1);
    assert_eq!(snapshot.omitted_error_count(), 1);
    assert_eq!(
        snapshot
            .first_error()
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range()
            .start(),
        ByteOffset::new(10)
    );
}
