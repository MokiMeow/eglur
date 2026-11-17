use std::cell::Cell;
use std::collections::BTreeSet;

use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticStore, PrimarySpan, Severity, SourceLabel, SourceSpan,
};
use eglur_hir::{HirFile, OriginId, lower};
use eglur_source::{FileId, LineIndex, SourceFile};
use eglur_syntax::parse_validated;

use super::*;
use crate::{ModuleAddress, SemanticSource, SourceInputError};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    pairs: usize,
    includes: usize,
    finishes: usize,
}

thread_local! {
    static WORK: Cell<Work> = Cell::new(Work::default());
}

pub(super) enum Operation {
    Pair,
    Include,
    Finish,
}

pub(super) fn observe(operation: Operation) {
    WORK.update(|mut work| {
        match operation {
            Operation::Pair => work.pairs += 1,
            Operation::Include => work.includes += 1,
            Operation::Finish => work.finishes += 1,
        }
        work
    });
}

fn roots(store: &mut DiagnosticStore, count: usize) -> Vec<Poison> {
    (0..count)
        .map(|index| {
            let diagnostic = Diagnostic::new(
                DiagnosticCode::new(format!("ROOT_{index:04}")).unwrap(),
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

fn union(poisons: &[Poison]) -> Poison {
    let mut result = poisons[0].clone();
    for poison in &poisons[1..] {
        result = result.merge(poison).unwrap();
    }
    result
}

fn assert_roots(
    mut store: DiagnosticStore,
    roots: &[Poison],
    poison: &Poison,
    expected: &BTreeSet<usize>,
) {
    store.select_preceding_roots(&roots[0], poison).unwrap();
    let retained = store
        .finalize(None)
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let expected = (0..roots.len())
        .filter(|index| !expected.contains(index))
        .map(|index| format!("ROOT_{index:04}"))
        .collect();
    assert_eq!(retained, expected);
}

struct Prepared {
    hir: HirFile,
    spans: Vec<(OriginId, SourceSpan)>,
    invalid: Vec<OriginId>,
}

fn prepare() -> Prepared {
    let source = SourceFile::new(
        FileId::new(0),
        b"fn first(: i64) -> Unit = ()\nfn second(: Bool) -> Unit = ()\nfn intact() -> Unit = ()\n"
            .to_vec(),
    );
    let validated = source.validate().unwrap();
    let parse = parse_validated(validated);
    let hir = lower(&parse);
    let lines = LineIndex::new(validated);
    let label = SourceLabel::new("main.eglur").unwrap();
    let spans = hir
        .origins()
        .map(|(origin, value)| {
            (
                origin,
                SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
            )
        })
        .collect();
    let invalid = hir
        .invalid_occurrences()
        .iter()
        .map(|occurrence| occurrence.origin())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    assert!(invalid.len() >= 2);

    Prepared {
        hir,
        spans,
        invalid,
    }
}

fn module() -> ModuleAddress {
    ModuleAddress::new([Box::<str>::from("main")]).unwrap()
}

#[test]
fn staged_groups_preserve_exact_distinct_shared_and_overlapping_roots() {
    for count in [1, 2, 3, 8, 32, 128] {
        for family in 0..3 {
            let mut store = DiagnosticStore::new();
            let roots = roots(&mut store, count + 3);
            let inputs = (0..count)
                .map(|index| match family {
                    0 => roots[index + 1].clone(),
                    1 => roots[1].clone(),
                    2 => roots[index + 1].merge(&roots[index + 2]).unwrap(),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>();
            let expected = match family {
                0 => (1..=count).collect(),
                1 => BTreeSet::from([1]),
                2 => (1..=count + 1).collect(),
                _ => unreachable!(),
            };

            WORK.set(Work::default());
            let mut group = InvalidPoisonGroup::One(inputs[0].clone());
            for poison in &inputs[1..] {
                group = group.include(poison.clone()).unwrap();
            }
            let poison = group.finish();
            let work = WORK.get();
            assert!(work.pairs <= 1);
            assert!(work.includes <= inputs.len());
            assert!(work.finishes <= 1);
            assert_eq!(work.pairs, usize::from(inputs.len() > 1));
            assert_eq!(work.finishes, usize::from(inputs.len() > 2));

            assert_roots(store, &roots, &poison, &expected);
        }
    }
}

#[test]
fn wide_singletons_and_pairs_keep_the_pairwise_kernel() {
    for count in [1, 2] {
        let mut store = DiagnosticStore::new();
        let roots = roots(&mut store, 129);
        let wide = union(&roots[1..]);

        WORK.set(Work::default());
        let mut group = InvalidPoisonGroup::One(wide.clone());
        for _ in 1..count {
            group = group.include(wide.clone()).unwrap();
        }
        let poison = group.finish();
        assert_eq!(
            WORK.get(),
            Work {
                pairs: count - 1,
                includes: 0,
                finishes: 0,
            }
        );
        assert_roots(store, &roots, &poison, &(1..roots.len()).collect());
    }
}

#[test]
fn source_keeps_interleaved_origin_groups_separate_and_preserves_other_fields() {
    let prepared = prepare();
    for count in [1, 2, 3, 8, 32] {
        for (focus_slot, focus_origin) in prepared.invalid.iter().enumerate() {
            let mut store = DiagnosticStore::new();
            let roots = roots(&mut store, 2 + count * prepared.invalid.len());
            let mut rows = Vec::new();
            for index in 0..count {
                for (slot, origin) in prepared.invalid.iter().enumerate().rev() {
                    rows.push((*origin, roots[1 + slot * count + index].clone()));
                }
            }

            WORK.set(Work::default());
            let source = SemanticSource::new(
                prepared.hir.file(),
                module(),
                Some(&prepared.hir),
                prepared.spans.clone(),
                rows,
                Some(roots.last().unwrap().clone()),
            )
            .unwrap();
            let work = WORK.get();
            assert!(work.pairs <= prepared.invalid.len());
            assert!(work.includes <= count * prepared.invalid.len());
            assert!(work.finishes <= prepared.invalid.len());
            assert_eq!(source.file(), prepared.hir.file());
            assert_eq!(source.module(), &module());
            assert!(std::ptr::eq(source.hir().unwrap(), &prepared.hir));
            for (origin, span) in &prepared.spans {
                assert_eq!(source.origin_span(*origin), Some(span));
                assert_eq!(
                    source.invalid_poison(*origin).is_some(),
                    prepared.invalid.contains(origin)
                );
            }
            store
                .record_failure(DiagnosticFailure::Derivative(
                    source.source_poison().unwrap().clone(),
                ))
                .unwrap();

            let expected = (1 + focus_slot * count..1 + (focus_slot + 1) * count).collect();
            assert_roots(
                store,
                &roots,
                source.invalid_poison(*focus_origin).unwrap(),
                &expected,
            );
        }
    }
}

#[test]
fn foreign_same_origin_stops_input_before_invalid_coverage_checks() {
    let prepared = prepare();
    for count in [2, 3, 8, 32] {
        let mut local = DiagnosticStore::new();
        let mut foreign = DiagnosticStore::new();
        let local = roots(&mut local, count);
        let foreign = roots(&mut foreign, 1);
        let mut rows = local[..count - 1]
            .iter()
            .cloned()
            .map(|poison| (prepared.invalid[0], poison))
            .collect::<Vec<_>>();
        rows.push((prepared.invalid[0], foreign[0].clone()));
        rows.push((prepared.invalid[0], local[0].clone()));
        let consumed = Cell::new(0);
        let rows = rows
            .into_iter()
            .inspect(|_| consumed.set(consumed.get() + 1));

        WORK.set(Work::default());
        let error = SemanticSource::new(
            prepared.hir.file(),
            module(),
            Some(&prepared.hir),
            prepared.spans.clone(),
            rows,
            None,
        )
        .err();
        assert_eq!(
            error,
            Some(SourceInputError::DiagnosticStore(
                DiagnosticStoreError::ForeignPoison
            ))
        );
        assert_eq!(consumed.get(), count);
        assert_eq!(WORK.get().finishes, 0);
    }
}

#[test]
fn unrelated_origins_and_source_poison_may_use_different_stores() {
    let prepared = prepare();
    let mut local = DiagnosticStore::new();
    let mut foreign = DiagnosticStore::new();
    let local_root = roots(&mut local, 1).remove(0);
    let foreign_root = roots(&mut foreign, 1).remove(0);
    let rows = prepared.invalid.iter().enumerate().map(|(index, origin)| {
        (
            *origin,
            if index % 2 == 0 {
                local_root.clone()
            } else {
                foreign_root.clone()
            },
        )
    });
    let source = SemanticSource::new(
        prepared.hir.file(),
        module(),
        Some(&prepared.hir),
        prepared.spans.clone(),
        rows,
        Some(foreign_root.clone()),
    )
    .unwrap();
    for (index, origin) in prepared.invalid.iter().enumerate() {
        let poison = source.invalid_poison(*origin).unwrap();
        assert_eq!(
            local
                .record_failure(DiagnosticFailure::Derivative(poison.clone()))
                .is_ok(),
            index % 2 == 0
        );
        assert_eq!(
            foreign
                .record_failure(DiagnosticFailure::Derivative(poison.clone()))
                .is_ok(),
            index % 2 != 0
        );
    }
    assert!(
        foreign
            .record_failure(DiagnosticFailure::Derivative(
                source.source_poison().unwrap().clone()
            ))
            .is_ok()
    );
    assert!(
        local
            .record_failure(DiagnosticFailure::Derivative(
                source.source_poison().unwrap().clone()
            ))
            .is_err()
    );
}

#[test]
fn coverage_failure_does_not_finalize_accumulating_groups() {
    let prepared = prepare();
    let mut store = DiagnosticStore::new();
    let roots = roots(&mut store, 8);
    let consumed = Cell::new(0);
    let rows = roots
        .iter()
        .cloned()
        .map(|poison| (prepared.invalid[1], poison))
        .inspect(|_| consumed.set(consumed.get() + 1));

    WORK.set(Work::default());
    let error = SemanticSource::new(
        prepared.hir.file(),
        module(),
        Some(&prepared.hir),
        prepared.spans,
        rows,
        None,
    )
    .err();
    assert_eq!(error, Some(SourceInputError::InvalidPoisonCoverage));
    assert_eq!(consumed.get(), roots.len());
    assert!(WORK.get().includes > 0);
    assert_eq!(WORK.get().finishes, 0);
}
